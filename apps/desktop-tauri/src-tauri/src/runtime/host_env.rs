//! Scan the host for a compatible Node / pnpm before any mirror download.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::config::{
    DEFAULT_NODE_VERSION, MIN_NODE_MINOR_FOR_22, MIN_UNRESTRICTED_NODE_MAJOR,
};
use super::env_path::{is_direct_spawnable_cli, which_on_host};
use super::process::hide_console;

/// Host toolchain selected for provisioning and Host startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostToolchain {
    pub node: Option<PathBuf>,
    pub pnpm: Option<PathBuf>,
}

/// Whether a `node --version` string satisfies `^22.19 || >=24`.
pub fn node_version_compatible(raw: &str) -> bool {
    let Some((major, minor, _)) = parse_semver(raw) else {
        return false;
    };
    (major == 22 && minor >= MIN_NODE_MINOR_FOR_22) || major >= MIN_UNRESTRICTED_NODE_MAJOR
}

/// Probe `binary --version` and return the first line when the process succeeds.
pub fn tool_version(binary: &Path) -> Option<String> {
    if !binary.is_file() {
        return None;
    }
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// True when `path` is an executable Node that satisfies the engine range.
pub fn node_binary_compatible(path: &Path) -> bool {
    tool_version(path)
        .map(|version| node_version_compatible(&version))
        .unwrap_or(false)
}

/// True when `path` is a spawnable pnpm that prints a version.
pub fn pnpm_binary_usable(path: &Path) -> bool {
    is_direct_spawnable_cli(path) && tool_version(path).is_some()
}

/// Scan PATH and well-known install locations after checking a preferred runtime.
pub fn scan_host_toolchain(preferred_node: &Path, preferred_pnpm: &Path) -> HostToolchain {
    let node = first_compatible_node(node_candidates(preferred_node));
    let mut pnpm_paths = pnpm_candidates(preferred_pnpm);
    if let Some(node) = &node {
        if let Some(dir) = node.parent() {
            pnpm_paths.extend(sibling_pnpm_paths(dir));
        }
    }
    let pnpm = first_usable_pnpm(dedup_paths(pnpm_paths));
    HostToolchain { node, pnpm }
}

/// Human-readable splash line after a toolchain scan.
pub fn toolchain_status(toolchain: &HostToolchain) -> String {
    match (&toolchain.node, &toolchain.pnpm) {
        (Some(node), Some(_)) => format!(
            "已匹配本机 Node {}，跳过运行时下载",
            tool_version(node).unwrap_or_else(|| format!("v{DEFAULT_NODE_VERSION}"))
        ),
        (Some(node), None) => format!(
            "已匹配本机 Node {}，将仅安装 pnpm",
            tool_version(node).unwrap_or_else(|| format!("v{DEFAULT_NODE_VERSION}"))
        ),
        (None, Some(_)) => "未找到兼容 Node，将下载运行时并复用本机 pnpm".into(),
        (None, None) => "未找到兼容 Node / pnpm，将从镜像下载".into(),
    }
}

fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .and_then(|part| part.split('-').next()?.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

fn first_compatible_node(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|path| node_binary_compatible(path))
}

fn first_usable_pnpm(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|path| pnpm_binary_usable(path))
}

fn node_candidates(preferred: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![preferred.to_path_buf()];
    push_which(&mut candidates, "node");
    candidates.extend(well_known_node_paths());
    dedup_paths(candidates)
}

fn pnpm_candidates(preferred: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![preferred.to_path_buf()];
    push_which(&mut candidates, "pnpm");
    #[cfg(windows)]
    push_which(&mut candidates, "pnpm.cmd");
    candidates.extend(well_known_pnpm_paths());
    dedup_paths(candidates)
}

fn push_which(candidates: &mut Vec<PathBuf>, name: &str) {
    if let Some(path) = which_on_host(name) {
        candidates.push(path);
    }
}

fn sibling_pnpm_paths(node_dir: &Path) -> Vec<PathBuf> {
    let mut paths = vec![node_dir.join("pnpm")];
    #[cfg(windows)]
    {
        paths.push(node_dir.join("pnpm.cmd"));
        paths.push(node_dir.join("pnpm.exe"));
    }
    paths
}

fn well_known_node_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        push_env_join(&mut paths, "ProgramFiles", &["nodejs", "node.exe"]);
        push_env_join(&mut paths, "ProgramFiles(x86)", &["nodejs", "node.exe"]);
        push_env_join(&mut paths, "NVM_SYMLINK", &["node.exe"]);
        push_env_join(&mut paths, "NVM_HOME", &["node.exe"]);
        push_home_join(&mut paths, &[".volta", "bin", "node.exe"]);
        push_home_join(&mut paths, &["scoop", "apps", "nodejs", "current", "node.exe"]);
        push_env_join(&mut paths, "LOCALAPPDATA", &["fnm", "aliases", "default", "node.exe"]);
    }
    #[cfg(not(windows))]
    {
        paths.push(PathBuf::from("/usr/local/bin/node"));
        paths.push(PathBuf::from("/usr/bin/node"));
        push_home_join(&mut paths, &[".volta", "bin", "node"]);
        push_home_join(&mut paths, &[".fnm", "current", "bin", "node"]);
        push_home_join(&mut paths, &[".nvm", "current", "bin", "node"]);
    }
    paths
}

fn well_known_pnpm_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        push_env_join(&mut paths, "LOCALAPPDATA", &["pnpm", "pnpm.exe"]);
        push_env_join(&mut paths, "LOCALAPPDATA", &["pnpm", "pnpm.cmd"]);
        push_env_join(&mut paths, "APPDATA", &["npm", "pnpm.cmd"]);
        push_env_join(&mut paths, "APPDATA", &["npm", "pnpm.exe"]);
        push_home_join(&mut paths, &[".volta", "bin", "pnpm.exe"]);
    }
    #[cfg(not(windows))]
    {
        paths.push(PathBuf::from("/usr/local/bin/pnpm"));
        paths.push(PathBuf::from("/usr/bin/pnpm"));
        push_home_join(&mut paths, &[".local", "share", "pnpm", "pnpm"]);
        push_home_join(&mut paths, &[".volta", "bin", "pnpm"]);
    }
    paths
}

fn push_env_join(paths: &mut Vec<PathBuf>, key: &str, suffix: &[&str]) {
    if let Ok(root) = std::env::var(key) {
        if !root.trim().is_empty() {
            paths.push(join_segments(PathBuf::from(root), suffix));
        }
    }
}

fn push_home_join(paths: &mut Vec<PathBuf>, suffix: &[&str]) {
    if let Some(home) = dirs::home_dir() {
        paths.push(join_segments(home, suffix));
    }
}

fn join_segments(mut root: PathBuf, suffix: &[&str]) -> PathBuf {
    for part in suffix {
        root.push(part);
    }
    root
}

fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::{node_version_compatible, parse_semver, pnpm_binary_usable, toolchain_status, HostToolchain};
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn rejects_a_powershell_pnpm_shim_as_unusable() {
        assert!(!pnpm_binary_usable(PathBuf::from(r"C:\Users\me\AppData\Roaming\npm\pnpm.ps1").as_path()));
    }

    #[test]
    fn accepts_engine_range_and_rejects_older_node() {
        assert!(node_version_compatible("v22.19.0"));
        assert!(node_version_compatible("22.20.1"));
        assert!(node_version_compatible("v24.4.0"));
        assert!(node_version_compatible("v25.0.0-nightly"));
        assert!(!node_version_compatible("v22.18.0"));
        assert!(!node_version_compatible("v20.19.0"));
        assert!(!node_version_compatible("v18.20.0"));
        assert!(!node_version_compatible("not-a-version"));
    }

    #[test]
    fn parses_optional_patch_and_prerelease() {
        assert_eq!(parse_semver("v22.19"), Some((22, 19, 0)));
        assert_eq!(parse_semver("24.1.2-rc.1"), Some((24, 1, 2)));
    }

    #[test]
    fn describes_scan_outcome_without_downloading() {
        let matched = HostToolchain {
            node: Some(PathBuf::from("node")),
            pnpm: Some(PathBuf::from("pnpm")),
        };
        assert!(toolchain_status(&matched).contains("跳过运行时下载"));
        let missing = HostToolchain {
            node: None,
            pnpm: None,
        };
        assert!(toolchain_status(&missing).contains("将从镜像下载"));
    }
}
