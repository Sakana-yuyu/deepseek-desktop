use std::fs::{self, File};
use std::io::{copy, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use flate2::read::GzDecoder;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tar::Archive;
use zip::ZipArchive;

use super::boot_log;
use super::config::{
    dev_launch_mode, node_mirror_base, npm_registry, DEFAULT_NODE_VERSION, DEFAULT_PNPM_VERSION,
    HARNESS_VERSIONS_DIR,
};
use super::host_env::{
    node_binary_compatible, pnpm_binary_usable, scan_host_toolchain, toolchain_status,
};
use super::io_fallback::{is_recoverable_io, recoverable_message};
use super::process::hide_console;
use super::user_home::{resolve_user_home, user_home_status};
use super::{app_data_root, ProvisionEvent};
use crate::i18n::{self, Msg};

/// Paths to the provisioned build environment and harness tree.
#[derive(Clone, Debug)]
pub struct RuntimePaths {
    pub node_binary: PathBuf,
    pub pnpm_binary: PathBuf,
    pub cli_entry: PathBuf,
    pub harness_root: PathBuf,
    pub runtime_root: PathBuf,
    pub dsh_home: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveKind {
    TarGz,
    Zip,
}

/// Provisioning step ceilings. Expiry fails the step into the recovery path
/// instead of parking the boot splash on a wedged network or subprocess.
const PNPM_HARNESS_INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const PNPM_GLOBAL_INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const NODE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Harness trees kept under `harness-versions`: the active tree, the fallback
/// for a failed provision, and one spare for an older still-running Host.
const HARNESS_TREES_KEPT: usize = 3;


/// Node distribution archive coordinates for a given OS/arch.
#[derive(Debug)]
pub(crate) struct NodeArchiveSpec {
    pub(crate) archive_name: String,
    pub(crate) inner_folder: String,
    kind: ArchiveKind,
    pub(crate) url: String,
}

/// Ensure bundled harness + Node + pnpm deps exist; mirror-fetch only build tools.
pub async fn ensure_runtime(
    bundled_source: Option<PathBuf>,
    progress: impl Fn(ProvisionEvent) + Send + Sync + 'static,
) -> Result<RuntimePaths, String> {
    if let Some(mode) = dev_launch_mode() {
        if mode == "local" || mode == "source" {
            progress(ProvisionEvent::Status(i18n::t(Msg::StatusLocalRepo).into()));
            progress(ProvisionEvent::Progress(100));
            return resolve_local_repo();
        }
    }

    let bundled = bundled_source.ok_or_else(|| i18n::t(Msg::BootMissingBundle).to_string())?;

    let runtime_root = app_data_root()?.join("runtime");
    let node_dir = runtime_root.join("node");
    let pnpm_home = runtime_root.join("pnpm-global");
    let app_root = app_data_root()?;
    let bundle_hash = read_bundle_hash(&bundled)?;
    let harness_root = harness_root_for_bundle(&app_root, &bundle_hash);
    let manifest_path = runtime_root.join("manifest.json");
    let isolated_home = app_data_root()?.join("dsh-home");

    let preferred_node = node_binary_path(&node_dir);
    let preferred_pnpm = pnpm_binary_path(&pnpm_home);
    let cli_entry = harness_root
        .join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js");

    progress(ProvisionEvent::Status(
        i18n::t(Msg::StatusMatchingHome).into(),
    ));
    progress(ProvisionEvent::Progress(8));
    let user_home = resolve_user_home(&isolated_home);
    let dsh_home = user_home.path.clone();
    progress(ProvisionEvent::Status(user_home_status(
        &user_home,
        &isolated_home,
    )));

    if manifest_ready(&manifest_path, &bundled, &harness_root, &cli_entry) {
        boot_log::info("provision skipped: manifest ready");
        progress(ProvisionEvent::Status(
            i18n::t(Msg::StatusRuntimeReady).into(),
        ));
        progress(ProvisionEvent::Progress(100));
        // The recorded node may be a host binary outside the desktop runtime
        // dir; reuse it so a skipped provision can still start the host.
        let node_binary = recorded_node_path(&manifest_path)
            .map(PathBuf::from)
            .unwrap_or(preferred_node);
        return Ok(RuntimePaths {
            node_binary,
            pnpm_binary: preferred_pnpm,
            cli_entry,
            harness_root,
            runtime_root,
            dsh_home,
        });
    }

    progress(ProvisionEvent::Status(
        i18n::t(Msg::StatusScanToolchain).into(),
    ));
    progress(ProvisionEvent::Progress(3));
    let toolchain = scan_host_toolchain(&preferred_node, &preferred_pnpm);
    progress(ProvisionEvent::Status(toolchain_status(&toolchain)));

    let mut node_binary = toolchain.node.unwrap_or_else(|| preferred_node.clone());
    let mut pnpm_binary = toolchain.pnpm.unwrap_or_else(|| preferred_pnpm.clone());

    boot_log::info("provision starting: seed harness + node + pnpm install");
    if let Err(error) = fs::create_dir_all(&runtime_root) {
        boot_log::info(&recoverable_message("create runtime", &runtime_root, error));
    }
    if let Err(error) = fs::create_dir_all(&dsh_home) {
        boot_log::info(&recoverable_message("create home", &dsh_home, error));
    }

    progress(ProvisionEvent::Status(
        i18n::t(Msg::StatusExtractHarness).into(),
    ));
    progress(ProvisionEvent::Progress(12));
    let mut harness_root = harness_root;
    let mut cli_entry = cli_entry;
    if let Err(error) = seed_harness_tree(&bundled, &harness_root) {
        boot_log::info(&format!("seed fallback: {error}"));
        if !cli_entry.is_file() {
            if let Some(existing) = find_existing_harness(&app_root) {
                boot_log::info(&format!("reusing harness {}", existing.display()));
                harness_root = existing;
                cli_entry = harness_root
                    .join("apps")
                    .join("cli")
                    .join("lib")
                    .join("bin.js");
            } else if !is_recoverable_io(&error) {
                return Err(error);
            }
        }
    }

    if node_binary_compatible(&node_binary) {
        boot_log::info(&format!("reusing Node {}", node_binary.display()));
        progress(ProvisionEvent::Status(
            i18n::t(Msg::StatusReusedNode).into(),
        ));
        progress(ProvisionEvent::Progress(30));
    } else {
        progress(ProvisionEvent::Status(i18n::tf(
            Msg::StatusDownloadNode,
            DEFAULT_NODE_VERSION,
        )));
        progress(ProvisionEvent::Progress(15));
        if let Err(error) = fetch_node(&node_dir, DEFAULT_NODE_VERSION, &progress).await {
            boot_log::info(&format!("node download fallback: {error}"));
            if preferred_node.is_file() {
                node_binary = preferred_node;
            } else if !is_recoverable_io(&error) {
                return Err(error);
            }
        } else {
            node_binary = preferred_node;
        }
    }

    if pnpm_binary_usable(&pnpm_binary) {
        boot_log::info(&format!("reusing pnpm {}", pnpm_binary.display()));
        progress(ProvisionEvent::Status(
            i18n::t(Msg::StatusReusedPnpm).into(),
        ));
        progress(ProvisionEvent::Progress(40));
    } else {
        progress(ProvisionEvent::Status(i18n::tf(
            Msg::StatusInstallPnpm,
            DEFAULT_PNPM_VERSION,
        )));
        progress(ProvisionEvent::Progress(35));
        if let Err(error) = install_pnpm(&node_binary, &pnpm_home, DEFAULT_PNPM_VERSION) {
            boot_log::info(&format!("pnpm install fallback: {error}"));
            if preferred_pnpm.is_file() {
                pnpm_binary = preferred_pnpm;
            } else if !is_recoverable_io(&error) {
                return Err(error);
            }
        } else {
            pnpm_binary = preferred_pnpm;
        }
    }

    progress(ProvisionEvent::Status(
        i18n::t(Msg::StatusInstallDeps).into(),
    ));
    progress(ProvisionEvent::Progress(50));
    if let Err(error) = pnpm_install_harness(&node_binary, &pnpm_binary, &harness_root) {
        boot_log::info(&format!("pnpm install harness fallback: {error}"));
        if !harness_root.join("node_modules").join(".pnpm").is_dir() && !is_recoverable_io(&error) {
            return Err(error);
        }
    }

    if !cli_entry.is_file() {
        return Err(format!(
            "harness CLI 缺失: {} — 请确认安装包内已包含 apps/cli/lib",
            cli_entry.display()
        ));
    }

    if let Err(error) = write_manifest(
        &manifest_path,
        &bundled,
        &node_binary,
        &harness_root,
        &cli_entry,
    ) {
        boot_log::info(&format!("manifest write skipped: {error}"));
    }
    gc_harness_versions(&app_root);

    progress(ProvisionEvent::Status(
        i18n::t(Msg::StatusRuntimeReady).into(),
    ));
    progress(ProvisionEvent::Progress(100));

    Ok(RuntimePaths {
        node_binary,
        pnpm_binary,
        cli_entry,
        harness_root,
        runtime_root,
        dsh_home,
    })
}

fn node_binary_path(node_dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        node_dir.join("node.exe")
    }
    #[cfg(not(windows))]
    {
        node_dir.join("bin").join("node")
    }
}

fn pnpm_binary_path(pnpm_home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        pnpm_home.join("pnpm.cmd")
    }
    #[cfg(not(windows))]
    {
        pnpm_home.join("bin").join("pnpm")
    }
}

fn resolve_local_repo() -> Result<RuntimePaths, String> {
    let repo = std::env::var("DSH_DESKTOP_REPO").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .canonicalize()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".into())
    });

    let harness_root = PathBuf::from(&repo);
    let cli_entry = harness_root
        .join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js");
    if !cli_entry.is_file() {
        return Err(format!(
            "本地 CLI 未构建: {} — 请在仓库根目录运行 pnpm run build",
            cli_entry.display()
        ));
    }

    let toolchain = scan_host_toolchain(Path::new("node"), Path::new("pnpm"));
    let node_binary = toolchain
        .node
        .ok_or_else(|| "找不到兼容 Node；请安装 Node ^22.19 或 >=24，或设置 PATH".to_string())?;
    let pnpm_binary = toolchain.pnpm.unwrap_or_else(|| {
        #[cfg(windows)]
        {
            PathBuf::from("pnpm.cmd")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("pnpm")
        }
    });

    let isolated_home = app_data_root()?.join("dsh-home");
    let dsh_home = resolve_user_home(&isolated_home).path;

    Ok(RuntimePaths {
        node_binary,
        pnpm_binary,
        cli_entry,
        harness_root,
        runtime_root: app_data_root()?.join("runtime"),
        dsh_home,
    })
}

fn manifest_ready(
    manifest_path: &Path,
    bundled: &Path,
    harness_root: &Path,
    cli_entry: &Path,
) -> bool {
    if !manifest_path.is_file()
        || !cli_entry.is_file()
        || !harness_root.join("node_modules").join(".pnpm").is_dir()
    {
        return false;
    }

    let Ok(raw) = fs::read_to_string(manifest_path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let Ok(bundle_hash) = read_bundle_hash(bundled) else {
        return false;
    };
    if bundle_hash != parsed["bundleSha256"].as_str().unwrap_or("") {
        return false;
    }

    // The recorded Node may be a host binary (e.g. an nvm-managed node.exe)
    // rather than the desktop-managed runtime node. Only the recorded path
    // proves the previous provision is still valid.
    let Some(node_path) = parsed["nodePath"].as_str() else {
        return false;
    };
    node_matches_manifest(Path::new(node_path), &parsed)
}

fn node_matches_manifest(node_binary: &Path, parsed: &serde_json::Value) -> bool {
    let Ok(meta) = fs::metadata(node_binary) else {
        return false;
    };
    if let Some(bytes) = parsed["nodeBytes"].as_u64() {
        return meta.len() == bytes;
    }
    true
}

/// The Node path recorded by the previous provision, if any.
fn recorded_node_path(manifest_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(manifest_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parsed["nodePath"].as_str().map(str::to_string)
}

/// Rebuild `RuntimePaths` from whatever Node / CLI already exists on disk.
pub fn try_recover_paths(bundled: Option<&Path>) -> Option<RuntimePaths> {
    let app_root = app_data_root().ok()?;
    let runtime_root = app_root.join("runtime");
    let isolated_home = app_root.join("dsh-home");
    let preferred_node = node_binary_path(&runtime_root.join("node"));
    let preferred_pnpm = pnpm_binary_path(&runtime_root.join("pnpm-global"));
    let toolchain = scan_host_toolchain(&preferred_node, &preferred_pnpm);
    let node_binary = toolchain
        .node
        .filter(|path| path.is_file())
        .or_else(|| preferred_node.is_file().then_some(preferred_node))?;
    let pnpm_binary = toolchain
        .pnpm
        .filter(|path| path.is_file())
        .or_else(|| preferred_pnpm.is_file().then_some(preferred_pnpm))?;
    let harness_root = bundled
        .and_then(|source| read_bundle_hash(source).ok())
        .map(|hash| harness_root_for_bundle(&app_root, &hash))
        .filter(|path| harness_tree_bootable(path))
        .or_else(|| find_existing_harness(&app_root))?;
    let cli_entry = harness_root
        .join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js");
    if !cli_entry.is_file() {
        return None;
    }
    let dsh_home = resolve_user_home(&isolated_home).path;
    Some(RuntimePaths {
        node_binary,
        pnpm_binary,
        cli_entry,
        harness_root,
        runtime_root,
        dsh_home,
    })
}

/// A harness tree boots the Host only when the prebuilt CLI entry and the
/// installed dependency store are both present. A freshly seeded tree always
/// ships `bin.js`, so the dependency store is what separates a bootable tree
/// from one whose `pnpm install` has not run (or failed).
fn harness_tree_bootable(root: &Path) -> bool {
    root.join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js")
        .is_file()
        && root.join("node_modules").join(".pnpm").is_dir()
}

fn mtime_of(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Order candidate harness trees newest first so recovery prefers the most
/// recently provisioned tree; equal timestamps fall back to name order.
fn sort_harness_trees_newest_first(dirs: &mut [PathBuf]) {
    dirs.sort_by(|a, b| {
        mtime_of(b)
            .cmp(&mtime_of(a))
            .then_with(|| b.cmp(a))
    });
}

fn find_existing_harness(app_root: &Path) -> Option<PathBuf> {
    let versions = app_root.join(HARNESS_VERSIONS_DIR);
    if let Ok(entries) = fs::read_dir(&versions) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && harness_tree_bootable(path))
            .collect();
        sort_harness_trees_newest_first(&mut dirs);
        if let Some(newest) = dirs.first() {
            return Some(newest.clone());
        }
    }
    let legacy = app_root.join("harness");
    harness_tree_bootable(&legacy).then_some(legacy)
}

/// Delete harness trees beyond the newest kept set. Removal failures are
/// logged and skipped: an older Host may still hold files open.
fn gc_harness_versions(app_root: &Path) {
    let versions = app_root.join(HARNESS_VERSIONS_DIR);
    let Ok(entries) = fs::read_dir(&versions) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    if dirs.len() <= HARNESS_TREES_KEPT {
        return;
    }
    sort_harness_trees_newest_first(&mut dirs);
    for stale in &dirs[HARNESS_TREES_KEPT..] {
        match fs::remove_dir_all(stale) {
            Ok(()) => boot_log::info(&format!("removed old harness {}", stale.display())),
            Err(error) => {
                boot_log::info(&format!(
                    "old harness removal skipped {}: {error}",
                    stale.display()
                ));
            }
        }
    }
}

pub(crate) fn read_bundle_hash(bundled: &Path) -> Result<String, String> {
    let manifest = bundled.join(".bundle-manifest.json");
    let raw = fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    parsed["contentSha256"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("invalid bundle manifest: {}", manifest.display()))
}

fn harness_root_for_bundle(app_root: &Path, bundle_hash: &str) -> PathBuf {
    let directory = bundle_hash.get(..16).unwrap_or(bundle_hash);
    app_root.join(HARNESS_VERSIONS_DIR).join(directory)
}

fn node_distribution_root(node_binary: &Path) -> PathBuf {
    let parent = node_binary.parent().unwrap_or(node_binary);
    #[cfg(windows)]
    {
        parent.to_path_buf()
    }
    #[cfg(not(windows))]
    {
        if parent.file_name().and_then(|name| name.to_str()) == Some("bin") {
            parent.parent().unwrap_or(parent).to_path_buf()
        } else {
            parent.to_path_buf()
        }
    }
}

fn find_npm_cli(node_binary: &Path) -> Option<PathBuf> {
    let root = node_distribution_root(node_binary);
    let bundled = root
        .join(npm_modules_dir())
        .join("npm")
        .join("bin")
        .join("npm-cli.js");
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

fn write_manifest(
    path: &Path,
    bundled: &Path,
    node_binary: &Path,
    harness_root: &Path,
    cli_entry: &Path,
) -> Result<(), String> {
    let doc = serde_json::json!({
        "bundleSha256": read_bundle_hash(bundled)?,
        "harnessVersion": read_bundle_version(bundled)?,
        "nodeVersion": DEFAULT_NODE_VERSION,
        "pnpmVersion": DEFAULT_PNPM_VERSION,
        "nodeBytes": fs::metadata(node_binary).map(|meta| meta.len()).unwrap_or(0),
        "nodePath": node_binary.display().to_string(),
        "cliSha256": file_sha256(cli_entry)?,
        "harnessRoot": harness_root.display().to_string(),
        "provisionedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "nodeMirror": node_mirror_base(),
        "npmRegistry": npm_registry(),
        "method": "bundled-source-pnpm-install",
    });
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&doc).unwrap()),
    )
    .map_err(|e| e.to_string())
}

fn read_bundle_version(bundled: &Path) -> Result<String, String> {
    let manifest = bundled.join(".bundle-manifest.json");
    let raw = fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(parsed["harnessVersion"]
        .as_str()
        .unwrap_or("unknown")
        .to_string())
}

fn seed_harness_tree(source: &Path, dest: &Path) -> Result<(), String> {
    let cli = dest.join("apps").join("cli").join("lib").join("bin.js");
    if dest.exists() {
        if let Err(error) = fs::remove_dir_all(dest) {
            let message = recoverable_message("seed remove", dest, error);
            if cli.is_file() {
                boot_log::info(&format!("{message}; reusing existing tree"));
                return Ok(());
            }
            return Err(message);
        }
    }
    match copy_tree(source, dest) {
        Ok(()) => Ok(()),
        Err(error) if cli.is_file() => {
            boot_log::info(&format!(
                "seed copy skipped {}; reusing {}",
                error,
                dest.display()
            ));
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn copy_tree(source: &Path, dest: &Path) -> Result<(), String> {
    if source.is_file() {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| recoverable_message("create", parent, e))?;
        }
        fs::copy(source, dest)
            .map_err(|e| format!("copy {} -> {}: {e}", source.display(), dest.display()))?;
        return Ok(());
    }

    fs::create_dir_all(dest).map_err(|e| recoverable_message("create", dest, e))?;
    for entry in fs::read_dir(source).map_err(|e| recoverable_message("read", source, e))? {
        let entry = entry.map_err(|e| recoverable_message("read", source, e))?;
        let name = entry.file_name();
        if name == "node_modules" {
            continue;
        }
        copy_tree(&entry.path(), &dest.join(name))?;
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("无法读取 {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn fetch_node(
    node_dir: &Path,
    version: &str,
    progress: &impl Fn(ProvisionEvent),
) -> Result<(), String> {
    let spec = node_archive_spec(version)?;
    let cache = app_data_root()?.join("cache");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let archive_path = cache.join(&spec.archive_name);

    if !archive_path.is_file() {
        download_file(&spec.url, &archive_path, 15, 30, progress).await?;
    }

    if node_dir.exists() {
        fs::remove_dir_all(node_dir).map_err(|e| e.to_string())?;
    }

    match spec.kind {
        ArchiveKind::Zip => extract_node_zip(&archive_path, node_dir, &spec.inner_folder)?,
        ArchiveKind::TarGz => extract_node_tar_gz(&archive_path, node_dir, &spec.inner_folder)?,
    }

    progress(ProvisionEvent::Progress(34));
    Ok(())
}

fn extract_node_zip(
    archive_path: &Path,
    node_dir: &Path,
    inner_folder: &str,
) -> Result<(), String> {
    fs::create_dir_all(node_dir).map_err(|e| e.to_string())?;
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let expected_root = Path::new(inner_folder);

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let entry_path = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe path in Node zip: {}", entry.name()))?;
        let Some(relative) = safe_archive_relative_path(&entry_path, expected_root)? else {
            continue;
        };
        let out = node_dir.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out_file = File::create(&out).map_err(|e| e.to_string())?;
            copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn extract_node_tar_gz(
    archive_path: &Path,
    node_dir: &Path,
    inner_folder: &str,
) -> Result<(), String> {
    let staging = node_dir.with_extension("extracting");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.set_preserve_permissions(true);
    let expected_root = Path::new(inner_folder);

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path().map_err(|e| e.to_string())?.into_owned();
        safe_archive_relative_path(&entry_path, expected_root)?;
        if !entry.unpack_in(&staging).map_err(|e| e.to_string())? {
            return Err(format!(
                "unsafe path in Node tar archive: {}",
                entry_path.display()
            ));
        }
    }

    let extracted = staging.join(inner_folder);
    if !extracted.is_dir() {
        return Err(format!(
            "Node archive is missing expected directory: {}",
            extracted.display()
        ));
    }
    fs::rename(&extracted, node_dir).map_err(|e| e.to_string())?;
    fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    Ok(())
}

fn safe_archive_relative_path(
    path: &Path,
    expected_root: &Path,
) -> Result<Option<PathBuf>, String> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("unsafe archive path: {}", path.display()));
    }

    let relative = path.strip_prefix(expected_root).map_err(|_| {
        format!(
            "archive entry is outside {}: {}",
            expected_root.display(),
            path.display()
        )
    })?;
    if relative.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(relative.to_path_buf()))
    }
}

fn node_archive_spec(version: &str) -> Result<NodeArchiveSpec, String> {
    node_archive_spec_for(version, std::env::consts::OS, std::env::consts::ARCH)
}

pub(crate) fn node_archive_spec_for(
    version: &str,
    os: &str,
    arch: &str,
) -> Result<NodeArchiveSpec, String> {
    let base = node_mirror_base().trim_end_matches('/').to_string();
    let (target, kind, extension) = match (os, arch) {
        ("windows", "x86_64") => ("win-x64", ArchiveKind::Zip, "zip"),
        ("windows", "x86") => ("win-x86", ArchiveKind::Zip, "zip"),
        ("macos", "x86_64") => ("darwin-x64", ArchiveKind::TarGz, "tar.gz"),
        ("macos", "aarch64") => ("darwin-arm64", ArchiveKind::TarGz, "tar.gz"),
        ("linux", "x86_64") => ("linux-x64", ArchiveKind::TarGz, "tar.gz"),
        ("linux", "aarch64") => ("linux-arm64", ArchiveKind::TarGz, "tar.gz"),
        _ => return Err(format!("unsupported Node runtime target: {os}-{arch}")),
    };
    let inner_folder = format!("node-v{version}-{target}");
    let archive_name = format!("{inner_folder}.{extension}");
    let url = format!("{base}/v{version}/{archive_name}");
    Ok(NodeArchiveSpec {
        archive_name,
        inner_folder,
        kind,
        url,
    })
}

pub(crate) async fn download_file(
    url: &str,
    dest: &Path,
    progress_start: u8,
    progress_end: u8,
    progress: &impl Fn(ProvisionEvent),
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("dsh-desktop/0.1")
        .timeout(NODE_DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("下载失败 {}: HTTP {}", url, response.status()));
    }

    let total = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        if let Some(total) = total {
            let frac = downloaded as f64 / total as f64;
            let pct = progress_start as f64 + frac * (progress_end - progress_start) as f64;
            progress(ProvisionEvent::Progress(pct as u8));
        }
    }

    Ok(())
}

/// Spawn `cmd` and wait for its exit status, killing it at `timeout`. The
/// child's stdio is inherited from this process.
fn wait_status_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
    label: &str,
) -> Result<std::process::ExitStatus, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{label} 启动失败: {e}"))?;
    wait_child_with_timeout(&mut child, timeout, label)
}

fn wait_child_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    label: &str,
) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{label} 超时（超过 {} 分钟）",
                timeout.as_secs() / 60
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn install_pnpm(node_binary: &Path, pnpm_home: &Path, version: &str) -> Result<(), String> {
    if pnpm_home.exists() {
        fs::remove_dir_all(pnpm_home).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(pnpm_home).map_err(|e| e.to_string())?;

    let spec = format!("pnpm@{version}");
    let registry = npm_registry();
    let status = if let Some(npm_cli) = find_npm_cli(node_binary) {
        let mut cmd = Command::new(node_binary);
        cmd.arg(&npm_cli)
            .arg("install")
            .arg("-g")
            .arg(&spec)
            .arg("--prefix")
            .arg(pnpm_home)
            .arg("--registry")
            .arg(&registry)
            .arg("--no-audit")
            .arg("--no-fund")
            .arg("--loglevel=error");
        add_node_to_path(&mut cmd, node_binary)?;
        hide_console(&mut cmd);
        wait_status_with_timeout(&mut cmd, PNPM_GLOBAL_INSTALL_TIMEOUT, "pnpm 安装")?
    } else {
        let npm = which::which("npm")
            .or_else(|_| which::which("npm.cmd"))
            .map_err(|_| {
                format!(
                    "找不到 npm-cli.js 或 npm，无法通过 {} 安装 pnpm",
                    node_binary.display()
                )
            })?;
        let mut cmd = Command::new(npm);
        cmd.arg("install")
            .arg("-g")
            .arg(&spec)
            .arg("--prefix")
            .arg(pnpm_home)
            .arg("--registry")
            .arg(&registry)
            .arg("--no-audit")
            .arg("--no-fund")
            .arg("--loglevel=error");
        add_node_to_path(&mut cmd, node_binary)?;
        hide_console(&mut cmd);
        wait_status_with_timeout(&mut cmd, PNPM_GLOBAL_INSTALL_TIMEOUT, "pnpm 安装")?
    };

    if !status.success() {
        return Err(format!(
            "npm install -g {spec} 失败 (exit {status}); registry={registry}"
        ));
    }

    Ok(())
}

#[cfg(windows)]
fn npm_modules_dir() -> &'static str {
    "node_modules"
}

#[cfg(not(windows))]
fn npm_modules_dir() -> &'static str {
    "lib/node_modules"
}

fn pnpm_entry_path(pnpm_home: &Path) -> PathBuf {
    pnpm_home
        .join(npm_modules_dir())
        .join("pnpm")
        .join("bin")
        .join("pnpm.cjs")
}

fn add_node_to_path(cmd: &mut Command, node_binary: &Path) -> Result<(), String> {
    let node_bin_dir = node_binary
        .parent()
        .ok_or_else(|| format!("Node binary has no parent: {}", node_binary.display()))?;
    let mut paths = vec![node_bin_dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let joined = std::env::join_paths(paths).map_err(|e| e.to_string())?;
    cmd.env("PATH", joined);
    Ok(())
}

fn configure_pnpm_install(
    cmd: &mut Command,
    node_binary: &Path,
    harness_root: &Path,
    registry: &str,
) -> Result<(), String> {
    cmd.arg("install")
        .arg("--prod")
        .arg("--no-frozen-lockfile")
        .arg("--registry")
        .arg(registry)
        .current_dir(harness_root)
        .env("NPM_CONFIG_REGISTRY", registry)
        .env("npm_config_registry", registry)
        .env_remove("CI")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    add_node_to_path(cmd, node_binary)?;
    hide_console(cmd);
    Ok(())
}

pub(crate) fn pnpm_js_entry(pnpm_binary: &Path) -> Option<PathBuf> {
    let parent = pnpm_binary.parent()?;
    let homes = [Some(parent), parent.parent()];
    for home in homes.into_iter().flatten() {
        let entry = pnpm_entry_path(home);
        if entry.is_file() {
            return Some(entry);
        }
    }
    None
}

fn pnpm_install_harness(
    node_binary: &Path,
    pnpm_binary: &Path,
    harness_root: &Path,
) -> Result<(), String> {
    let registry = npm_registry();
    let mut cmd = if let Some(entry) = pnpm_js_entry(pnpm_binary) {
        let mut cmd = Command::new(node_binary);
        cmd.arg(entry);
        cmd
    } else if pnpm_binary_usable(pnpm_binary) {
        Command::new(pnpm_binary)
    } else {
        return Err(format!("pnpm entry is missing: {}", pnpm_binary.display()));
    };
    configure_pnpm_install(&mut cmd, node_binary, harness_root, &registry)?;

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("pnpm install 启动失败: {e}"))?;
    // Drain the pipes on dedicated threads: a chatty install would otherwise
    // fill the OS pipe buffer and block the child before a deadline can fire.
    let stdout_handle = spawn_pipe_reader(child.stdout.take());
    let stderr_handle = spawn_pipe_reader(child.stderr.take());
    let status =
        wait_child_with_timeout(&mut child, PNPM_HARNESS_INSTALL_TIMEOUT, "pnpm install")?;
    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        return Err(format!(
            "pnpm install 失败 (exit {})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            status
        ));
    }

    Ok(())
}

fn spawn_pipe_reader<T: Read + Send + 'static>(
    pipe: Option<T>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut buffer);
        }
        buffer
    })
}

#[cfg(test)]
mod tests {
    use super::{
        find_existing_harness, gc_harness_versions, harness_root_for_bundle,
        harness_tree_bootable, manifest_ready, node_archive_spec_for, node_matches_manifest,
        safe_archive_relative_path, HARNESS_TREES_KEPT,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn make_harness_tree(root: &Path, installed: bool) {
        let cli = root.join("apps").join("cli").join("lib").join("bin.js");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        fs::write(&cli, b"// cli").unwrap();
        if installed {
            fs::create_dir_all(root.join("node_modules").join(".pnpm")).unwrap();
        }
    }

    #[test]
    fn harness_tree_bootable_requires_installed_dependencies() {
        let dir = std::env::temp_dir().join(format!("dsh-bootable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let seeded = dir.join("seeded");
        make_harness_tree(&seeded, false);
        assert!(!harness_tree_bootable(&seeded));
        let installed = dir.join("installed");
        make_harness_tree(&installed, true);
        assert!(harness_tree_bootable(&installed));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_skips_a_tree_without_installed_dependencies() {
        let base = std::env::temp_dir().join(format!("dsh-recover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let versions = base.join("harness-versions");
        make_harness_tree(&versions.join("7c5e4321f834a90b"), false);
        make_harness_tree(&versions.join("7a9222660fa6f5d1"), true);
        let picked = find_existing_harness(&base).unwrap();
        assert!(picked.ends_with("7a9222660fa6f5d1"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn recovery_prefers_the_newest_bootable_tree() {
        let base = std::env::temp_dir().join(format!("dsh-recover-newest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let versions = base.join("harness-versions");
        make_harness_tree(&versions.join("older"), true);
        std::thread::sleep(Duration::from_millis(50));
        make_harness_tree(&versions.join("newer"), true);
        let picked = find_existing_harness(&base).unwrap();
        assert!(picked.ends_with("newer"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn gc_keeps_only_the_newest_harness_trees() {
        let base = std::env::temp_dir().join(format!("dsh-gc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let versions = base.join("harness-versions");
        for name in ["a", "b", "c", "d", "e"] {
            make_harness_tree(&versions.join(name), true);
            std::thread::sleep(Duration::from_millis(30));
        }
        gc_harness_versions(&base);
        let remaining: Vec<String> = fs::read_dir(&versions)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(remaining.len(), HARNESS_TREES_KEPT);
        for name in &remaining {
            assert!(["c", "d", "e"].contains(&name.as_str()), "kept {name}");
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn isolates_harness_trees_by_bundle_hash() {
        let app_root = Path::new("app-data");
        assert_eq!(
            harness_root_for_bundle(app_root, "0123456789abcdefaaaaaaaaaaaaaaaa"),
            PathBuf::from("app-data")
                .join("harness-versions")
                .join("0123456789abcdef")
        );
        assert_ne!(
            harness_root_for_bundle(app_root, "0123456789abcdefaaaaaaaaaaaaaaaa"),
            harness_root_for_bundle(app_root, "fedcba9876543210bbbbbbbbbbbbbbbb")
        );
    }

    #[test]
    fn selects_node_archive_for_every_release_target() {
        let cases = [
            ("windows", "x86_64", "win-x64", "zip"),
            ("windows", "x86", "win-x86", "zip"),
            ("macos", "x86_64", "darwin-x64", "tar.gz"),
            ("macos", "aarch64", "darwin-arm64", "tar.gz"),
            ("linux", "x86_64", "linux-x64", "tar.gz"),
            ("linux", "aarch64", "linux-arm64", "tar.gz"),
        ];

        for (os, arch, node_target, extension) in cases {
            let spec = node_archive_spec_for("22.19.0", os, arch).unwrap();
            assert_eq!(
                spec.archive_name,
                format!("node-v22.19.0-{node_target}.{extension}")
            );
            assert_eq!(spec.inner_folder, format!("node-v22.19.0-{node_target}"));
        }
    }

    #[test]
    fn rejects_unsupported_node_archive_targets() {
        let error = node_archive_spec_for("22.19.0", "linux", "x86").unwrap_err();
        assert!(error.contains("unsupported"));
    }

    #[test]
    fn accepts_only_archive_paths_below_expected_root() {
        assert_eq!(
            safe_archive_relative_path(
                Path::new("node-v22.19.0-linux-x64/bin/node"),
                Path::new("node-v22.19.0-linux-x64"),
            )
            .unwrap(),
            Some(Path::new("bin/node").to_path_buf())
        );
        assert_eq!(
            safe_archive_relative_path(
                Path::new("node-v22.19.0-linux-x64"),
                Path::new("node-v22.19.0-linux-x64"),
            )
            .unwrap(),
            None
        );
        assert!(safe_archive_relative_path(
            Path::new("node-v22.19.0-linux-x64/../../escape"),
            Path::new("node-v22.19.0-linux-x64"),
        )
        .is_err());
        assert!(safe_archive_relative_path(
            Path::new("../node-v22.19.0-linux-x64/bin/node"),
            Path::new("node-v22.19.0-linux-x64"),
        )
        .is_err());
    }

    #[test]
    fn treats_node_byte_size_as_manifest_identity() {
        let dir = std::env::temp_dir().join(format!("dsh-node-manifest-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let node = dir.join("node.exe");
        fs::write(&node, b"node-binary").unwrap();
        let bytes = fs::metadata(&node).unwrap().len();
        assert!(node_matches_manifest(
            &node,
            &serde_json::json!({ "nodeBytes": bytes })
        ));
        assert!(!node_matches_manifest(
            &node,
            &serde_json::json!({ "nodeBytes": bytes + 1 })
        ));
        assert!(node_matches_manifest(&node, &serde_json::json!({})));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_provision_when_recorded_host_node_still_matches() {
        let dir = std::env::temp_dir().join(format!("dsh-manifest-ready-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let harness = dir.join("harness");
        let node_modules = harness.join("node_modules").join(".pnpm");
        let _ = fs::create_dir_all(&node_modules);
        let cli = harness.join("apps").join("cli").join("lib").join("bin.js");
        let _ = fs::create_dir_all(cli.parent().unwrap());
        fs::write(&cli, b"cli").unwrap();

        let node = dir.join("node.exe");
        fs::write(&node, b"node-binary").unwrap();
        let bytes = fs::metadata(&node).unwrap().len();

        let bundled = dir.join("bundled");
        let _ = fs::create_dir_all(&bundled);
        fs::write(
            bundled.join(".bundle-manifest.json"),
            r#"{"contentSha256":"0123456789abcdef0123456789abcdef"}"#,
        )
        .unwrap();

        let manifest_path = dir.join("manifest.json");
        fs::write(
            &manifest_path,
            format!(
                r#"{{"bundleSha256":"0123456789abcdef0123456789abcdef","nodePath":"{}","nodeBytes":{}}}"#,
                node.display(),
                bytes
            ),
        )
        .unwrap();

        assert!(manifest_ready(
            &manifest_path,
            &bundled,
            &harness,
            &cli,
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reprovisions_when_recorded_node_is_missing() {
        let dir = std::env::temp_dir().join(format!("dsh-manifest-stale-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let harness = dir.join("harness");
        let node_modules = harness.join("node_modules").join(".pnpm");
        let _ = fs::create_dir_all(&node_modules);
        let cli = harness.join("apps").join("cli").join("lib").join("bin.js");
        let _ = fs::create_dir_all(cli.parent().unwrap());
        fs::write(&cli, b"cli").unwrap();

        let bundled = dir.join("bundled");
        let _ = fs::create_dir_all(&bundled);
        fs::write(
            bundled.join(".bundle-manifest.json"),
            r#"{"contentSha256":"0123456789abcdef0123456789abcdef"}"#,
        )
        .unwrap();

        // The recorded node no longer exists (e.g. an nvm version removed).
        let manifest_path = dir.join("manifest.json");
        fs::write(
            &manifest_path,
            format!(
                r#"{{"bundleSha256":"0123456789abcdef0123456789abcdef","nodePath":"{}","nodeBytes":1234}}}"#,
                dir.join("missing-node.exe").display()
            ),
        )
        .unwrap();

        assert!(!manifest_ready(
            &manifest_path,
            &bundled,
            &harness,
            &cli,
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
