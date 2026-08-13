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
use super::process::hide_console;
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

#[derive(Debug)]
struct NodeArchiveSpec {
    archive_name: String,
    inner_folder: String,
    kind: ArchiveKind,
    url: String,
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

    let node_binary = node_binary_path(&node_dir);
    let pnpm_binary = pnpm_binary_path(&pnpm_home);
    let cli_entry = harness_root
        .join("apps")
        .join("cli")
        .join("lib")
        .join("bin.js");
    let dsh_home = app_data_root()?.join("dsh-home");

    if manifest_ready(
        &manifest_path,
        &bundled,
        &node_binary,
        &pnpm_binary,
        &harness_root,
        &cli_entry,
    )? {
        boot_log::info("provision skipped: manifest ready");
        progress(ProvisionEvent::Status("运行环境已就绪".into()));
        progress(ProvisionEvent::Progress(100));
        return Ok(RuntimePaths {
            node_binary,
            pnpm_binary,
            cli_entry,
            harness_root,
            runtime_root,
            dsh_home,
        });
    }

    boot_log::info("provision starting: seed harness + node + pnpm install");
    fs::create_dir_all(&runtime_root).map_err(|e| e.to_string())?;
    fs::create_dir_all(&dsh_home).map_err(|e| e.to_string())?;

    progress(ProvisionEvent::Status("正在释放 harness 源码…".into()));
    progress(ProvisionEvent::Progress(5));
    seed_harness_tree(&bundled, &harness_root)?;

    progress(ProvisionEvent::Status(format!(
        "正在从镜像下载 Node {}…",
        DEFAULT_NODE_VERSION
    )));
    progress(ProvisionEvent::Progress(15));
    if node_runtime_ready(&node_binary) {
        boot_log::info("reusing installed Node runtime");
    } else {
        fetch_node(&node_dir, DEFAULT_NODE_VERSION, &progress).await?;
    }

    progress(ProvisionEvent::Status(format!(
        "正在从镜像安装 pnpm {}…",
        DEFAULT_PNPM_VERSION
    )));
    progress(ProvisionEvent::Progress(35));
    if pnpm_binary.is_file() {
        boot_log::info("reusing installed pnpm runtime");
    } else {
        install_pnpm(&node_binary, &node_dir, &pnpm_home, DEFAULT_PNPM_VERSION)?;
    }

    progress(ProvisionEvent::Status(
        "正在从镜像安装依赖 (pnpm install --prod --no-frozen-lockfile)…".into(),
    ));
    progress(ProvisionEvent::Progress(50));
    pnpm_install_harness(&node_binary, &pnpm_binary, &harness_root)?;

    if !cli_entry.is_file() {
        return Err(format!(
            "harness CLI 缺失: {} — 请确认安装包内已包含 apps/cli/lib",
            cli_entry.display()
        ));
    }

    write_manifest(
        &manifest_path,
        &bundled,
        &node_binary,
        &harness_root,
        &cli_entry,
    )?;

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

    let node_binary = which::which("node")
        .map_err(|_| "找不到 node；请安装 Node ^22.19 或设置 PATH".to_string())?;

    let pnpm_binary = which::which("pnpm").unwrap_or_else(|_| {
        #[cfg(windows)]
        {
            PathBuf::from("pnpm.cmd")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("pnpm")
        }
    });

    let dsh_home = app_data_root()?.join("dsh-home");
    fs::create_dir_all(&dsh_home).ok();

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
) -> Result<bool, String> {
    if !manifest_path.is_file()
        || !node_binary.is_file()
        || !pnpm_binary.is_file()
        || !cli_entry.is_file()
        || !harness_root.join("node_modules").join(".pnpm").is_dir()
    {
        return Ok(false);
    }

    let raw = fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let bundle_ok = read_bundle_hash(bundled)? == parsed["bundleSha256"].as_str().unwrap_or("");
    let node_ok = file_sha256(node_binary)? == parsed["nodeSha256"].as_str().unwrap_or("");
    Ok(bundle_ok && node_ok)
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

fn node_runtime_ready(node_binary: &Path) -> bool {
    if !node_binary.is_file() {
        return false;
    }
    let mut command = Command::new(node_binary);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut command);
    command
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim()
                    == format!("v{DEFAULT_NODE_VERSION}")
        })
        .unwrap_or(false)
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
        "nodeSha256": file_sha256(node_binary)?,
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
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    copy_tree(source, dest)?;
    Ok(())
}

fn copy_tree(source: &Path, dest: &Path) -> Result<(), String> {
    if source.is_file() {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(source, dest).map_err(|e| e.to_string())?;
        return Ok(());
    }

    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == "node_modules" {
            continue;
        }
        copy_tree(&entry.path(), &dest.join(name))?;
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
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

fn node_archive_spec_for(version: &str, os: &str, arch: &str) -> Result<NodeArchiveSpec, String> {
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

async fn download_file(
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

fn install_pnpm(
    node_binary: &Path,
    node_dir: &Path,
    pnpm_home: &Path,
    version: &str,
) -> Result<(), String> {
    if pnpm_home.exists() {
        fs::remove_dir_all(pnpm_home).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(pnpm_home).map_err(|e| e.to_string())?;

    let npm_cli = node_dir
        .join(npm_modules_dir())
        .join("npm")
        .join("bin")
        .join("npm-cli.js");

    let spec = format!("pnpm@{version}");
    let registry = npm_registry();

    let status = {
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

fn pnpm_install_harness(
    node_binary: &Path,
    pnpm_binary: &Path,
    harness_root: &Path,
) -> Result<(), String> {
    let registry = npm_registry();
    let pnpm_home = pnpm_binary
        .parent()
        .and_then(|parent| {
            #[cfg(windows)]
            {
                Some(parent)
            }
            #[cfg(not(windows))]
            {
                parent.parent()
            }
        })
        .ok_or_else(|| format!("invalid pnpm binary path: {}", pnpm_binary.display()))?;
    let pnpm_entry = pnpm_entry_path(pnpm_home);
    if !pnpm_entry.is_file() {
        return Err(format!("pnpm entry is missing: {}", pnpm_entry.display()));
    }

    let mut cmd = Command::new(node_binary);
    cmd.arg(&pnpm_entry);
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
    use super::{harness_root_for_bundle, node_archive_spec_for, safe_archive_relative_path};
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
}
