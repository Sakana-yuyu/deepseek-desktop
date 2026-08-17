use std::fs::{self, File};
use std::io::{copy, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

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
use super::process::hide_console;
use super::io_fallback::{is_recoverable_io, recoverable_message};
use super::user_home::{resolve_user_home, user_home_status};
use super::{app_data_root, ProvisionEvent};

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
            progress(ProvisionEvent::Status("使用本地仓库…".into()));
            progress(ProvisionEvent::Progress(100));
            return resolve_local_repo();
        }
    }

    let bundled = bundled_source
        .ok_or_else(|| "安装包内缺少 harness 源码资源；请重新构建 desktop-tauri".to_string())?;

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

    progress(ProvisionEvent::Status("正在匹配已有对话与密钥…".into()));
    progress(ProvisionEvent::Progress(8));
    let user_home = resolve_user_home(&isolated_home);
    let dsh_home = user_home.path.clone();
    progress(ProvisionEvent::Status(user_home_status(
        &user_home,
        &isolated_home,
    )));

    if manifest_ready(
        &manifest_path,
        &bundled,
        &preferred_node,
        &preferred_pnpm,
        &harness_root,
        &cli_entry,
    ) {
        boot_log::info("provision skipped: manifest ready");
        progress(ProvisionEvent::Status("运行环境已就绪".into()));
        progress(ProvisionEvent::Progress(100));
        return Ok(RuntimePaths {
            node_binary: preferred_node,
            pnpm_binary: preferred_pnpm,
            cli_entry,
            harness_root,
            runtime_root,
            dsh_home,
        });
    }

    progress(ProvisionEvent::Status("正在扫描本机 Node / pnpm…".into()));
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

    progress(ProvisionEvent::Status("正在释放 harness 源码…".into()));
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
        progress(ProvisionEvent::Status("已复用本机 Node，跳过下载".into()));
        progress(ProvisionEvent::Progress(30));
    } else {
        progress(ProvisionEvent::Status(format!(
            "正在从镜像下载 Node {}…",
            DEFAULT_NODE_VERSION
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
        progress(ProvisionEvent::Status("已复用本机 pnpm".into()));
        progress(ProvisionEvent::Progress(40));
    } else {
        progress(ProvisionEvent::Status(format!(
            "正在安装 pnpm {}…",
            DEFAULT_PNPM_VERSION
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
        "正在从镜像安装依赖 (pnpm install --prod --no-frozen-lockfile)…".into(),
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

    progress(ProvisionEvent::Status("运行环境已就绪".into()));
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
    let node_binary = toolchain.node.ok_or_else(|| {
        "找不到兼容 Node；请安装 Node ^22.19 或 >=24，或设置 PATH".to_string()
    })?;
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
    node_binary: &Path,
    pnpm_binary: &Path,
    harness_root: &Path,
    cli_entry: &Path,
) -> bool {
    if !manifest_path.is_file()
        || !node_binary.is_file()
        || !pnpm_binary.is_file()
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
    bundle_hash == parsed["bundleSha256"].as_str().unwrap_or("")
        && node_matches_manifest(node_binary, &parsed)
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

/// Rebuild `RuntimePaths` from whatever Node / CLI already exists on disk.
pub fn try_recover_paths(bundled: Option<&Path>) -> Option<RuntimePaths> {
    let app_root = app_data_root().ok()?;
    let runtime_root = app_root.join("runtime");
    let isolated_home = app_root.join("dsh-home");
    let preferred_node = node_binary_path(&runtime_root.join("node"));
    let preferred_pnpm = pnpm_binary_path(&runtime_root.join("pnpm-global"));
    let toolchain = scan_host_toolchain(&preferred_node, &preferred_pnpm);
    let node_binary = toolchain.node.filter(|path| path.is_file()).or_else(|| {
        preferred_node.is_file().then_some(preferred_node)
    })?;
    let pnpm_binary = toolchain.pnpm.filter(|path| path.is_file()).or_else(|| {
        preferred_pnpm.is_file().then_some(preferred_pnpm)
    })?;
    let harness_root = bundled
        .and_then(|source| read_bundle_hash(source).ok())
        .map(|hash| harness_root_for_bundle(&app_root, &hash))
        .filter(|path| {
            path.join("apps")
                .join("cli")
                .join("lib")
                .join("bin.js")
                .is_file()
        })
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

fn find_existing_harness(app_root: &Path) -> Option<PathBuf> {
    let versions = app_root.join(HARNESS_VERSIONS_DIR);
    if let Ok(entries) = fs::read_dir(&versions) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        dirs.sort();
        dirs.reverse();
        for dir in dirs {
            if dir
                .join("apps")
                .join("cli")
                .join("lib")
                .join("bin.js")
                .is_file()
            {
                return Some(dir);
            }
        }
    }
    let legacy = app_root.join("harness");
    legacy
        .join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js")
        .is_file()
        .then_some(legacy)
}

fn read_bundle_hash(bundled: &Path) -> Result<String, String> {
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
        fs::copy(source, dest).map_err(|e| {
            format!("copy {} -> {}: {e}", source.display(), dest.display())
        })?;
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
        cmd.status()
            .map_err(|e| format!("pnpm 安装启动失败: {e}"))?
    } else {
        let npm = which::which("npm").or_else(|_| which::which("npm.cmd")).map_err(|_| {
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
        cmd.status()
            .map_err(|e| format!("pnpm 安装启动失败: {e}"))?
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
        return Err(format!(
            "pnpm entry is missing: {}",
            pnpm_binary.display()
        ));
    };
    configure_pnpm_install(&mut cmd, node_binary, harness_root, &registry)?;

    let output = cmd
        .output()
        .map_err(|e| format!("pnpm install 启动失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "pnpm install 失败 (exit {})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        harness_root_for_bundle, node_archive_spec_for, node_matches_manifest,
        safe_archive_relative_path,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

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
        let dir = std::env::temp_dir().join(format!(
            "dsh-node-manifest-{}",
            std::process::id()
        ));
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
}
