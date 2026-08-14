//! Put the Host's required CLIs on PATH: process env first, user PATH when missing.

use std::fs;
use std::path::{Path, PathBuf};

use super::boot_log;
use super::env_path::{discovery_path, path_eq, which_on_host};
use super::provision::RuntimePaths;
use super::{app_data_root, ProvisionEvent};

#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
use super::process::hide_console;

const BIN_DIR_NAME: &str = "bin";

#[cfg(not(windows))]
const PROFILE_MARKER: &str = "dsh-desktop-path";

/// Directories the Host and its children must see on PATH.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathBridge {
    pub bin_dir: PathBuf,
    pub node_dir: Option<PathBuf>,
    pub pnpm_dir: Option<PathBuf>,
    pub prepend: Vec<PathBuf>,
}

/// Write `dsh` shims and return a PATH that includes every required CLI directory.
pub fn prepare_host_path(
    paths: &RuntimePaths,
    progress: impl Fn(ProvisionEvent),
) -> Result<String, String> {
    progress(ProvisionEvent::Status(
        "正在写入 dsh 命令并加入 PATH…".into(),
    ));
    let bridge = install_path_bridge(paths)?;
    if let Err(error) = persist_user_path(&bridge) {
        boot_log::info(&format!("user PATH persist skipped: {error}"));
    }
    let merged = merge_path(Some(discovery_path()), &bridge.prepend);
    boot_log::info(&format!(
        "path bridge bin={} prepend={}",
        bridge.bin_dir.display(),
        bridge
            .prepend
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(";")
    ));
    Ok(merged)
}

/// Create shims and collect directories that must precede the inherited PATH.
pub fn install_path_bridge(paths: &RuntimePaths) -> Result<PathBridge, String> {
    let bin_dir = app_data_root()?.join(BIN_DIR_NAME);
    fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    write_dsh_shims(&bin_dir, &paths.node_binary, &paths.cli_entry)?;

    let node_dir = paths.node_binary.parent().map(Path::to_path_buf);
    let pnpm_dir = paths.pnpm_binary.parent().map(Path::to_path_buf);
    let mut prepend = vec![bin_dir.clone()];
    push_unique_dir(&mut prepend, node_dir.as_deref());
    push_unique_dir(&mut prepend, pnpm_dir.as_deref());
    for dir in companion_tool_dirs() {
        push_unique_dir(&mut prepend, Some(&dir));
    }

    Ok(PathBridge {
        bin_dir,
        node_dir,
        pnpm_dir,
        prepend,
    })
}

/// Prepend unique directories onto an existing PATH value.
pub fn merge_path(existing: Option<impl AsRef<std::ffi::OsStr>>, prepend: &[PathBuf]) -> String {
    let mut parts = Vec::new();
    for dir in prepend {
        if !path_list_contains(&parts, dir) {
            parts.push(dir.clone());
        }
    }
    if let Some(existing) = existing {
        for dir in std::env::split_paths(existing.as_ref()) {
            if !dir.as_os_str().is_empty() && !path_list_contains(&parts, &dir) {
                parts.push(dir);
            }
        }
    }
    std::env::join_paths(parts)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|_| prepend[0].display().to_string())
}

fn write_dsh_shims(bin_dir: &Path, node: &Path, cli_entry: &Path) -> Result<(), String> {
    let node = quote_for_cmd(node);
    let cli = quote_for_cmd(cli_entry);

    #[cfg(windows)]
    {
        fs::write(
            bin_dir.join("dsh.cmd"),
            format!("@echo off\r\n{node} {cli} %*\r\n"),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            bin_dir.join("dsh"),
            format!("#!/bin/sh\nexec {node} {cli} \"$@\"\n"),
        )
        .map_err(|e| e.to_string())?;
    }

    #[cfg(not(windows))]
    {
        let script = bin_dir.join("dsh");
        fs::write(&script, format!("#!/bin/sh\nexec {node} {cli} \"$@\"\n"))
            .map_err(|e| e.to_string())?;
        set_executable(&script)?;
    }

    Ok(())
}

fn companion_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if which_on_host("git").is_none() {
        dirs.extend(existing_tool_dirs(well_known_git_dirs(), "git"));
    }
    if which_on_host("bash").is_none() {
        dirs.extend(existing_tool_dirs(well_known_bash_dirs(), "bash"));
    }
    dirs
}

fn well_known_git_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        push_env_join(&mut dirs, "ProgramFiles", &["Git", "cmd"]);
        push_env_join(&mut dirs, "ProgramFiles", &["Git", "bin"]);
        push_env_join(&mut dirs, "ProgramFiles(x86)", &["Git", "cmd"]);
        push_env_join(&mut dirs, "ProgramFiles(x86)", &["Git", "bin"]);
        push_env_join(&mut dirs, "LOCALAPPDATA", &["Programs", "Git", "cmd"]);
        push_env_join(&mut dirs, "LOCALAPPDATA", &["Programs", "Git", "bin"]);
    }
    dirs
}

fn well_known_bash_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        push_env_join(&mut dirs, "ProgramFiles", &["Git", "bin"]);
        push_env_join(&mut dirs, "ProgramFiles", &["Git", "usr", "bin"]);
        push_env_join(&mut dirs, "ProgramFiles(x86)", &["Git", "bin"]);
        push_env_join(&mut dirs, "LOCALAPPDATA", &["Programs", "Git", "bin"]);
    }
    dirs
}

fn existing_tool_dirs(candidates: Vec<PathBuf>, name: &str) -> Vec<PathBuf> {
    candidates
        .into_iter()
        .filter(|dir| tool_exists_in(dir, name))
        .collect()
}

fn tool_exists_in(dir: &Path, name: &str) -> bool {
    let mut names = vec![name.to_string()];
    #[cfg(windows)]
    {
        names.push(format!("{name}.exe"));
        names.push(format!("{name}.cmd"));
    }
    names.iter().any(|file| dir.join(file).is_file())
}

fn persist_user_path(bridge: &PathBridge) -> Result<(), String> {
    #[cfg(windows)]
    {
        persist_windows_user_path(&bridge.bin_dir)?;
        persist_windows_user_path_if_missing(bridge.node_dir.as_deref(), &["node"])?;
        persist_windows_user_path_if_missing(bridge.pnpm_dir.as_deref(), &["pnpm"])?;
    }
    #[cfg(not(windows))]
    {
        persist_unix_user_shim(&bridge.bin_dir)?;
    }
    Ok(())
}

#[cfg(windows)]
fn persist_windows_user_path_if_missing(
    dir: Option<&Path>,
    names: &[&str],
) -> Result<(), String> {
    let Some(dir) = dir else {
        return Ok(());
    };
    if names.iter().any(|name| which_on_host(name).is_some()) {
        return Ok(());
    }
    persist_windows_user_path(dir)
}

#[cfg(windows)]
fn persist_windows_user_path(dir: &Path) -> Result<(), String> {
    let current = read_windows_user_path()?;
    let dir_text = dir.display().to_string();
    if path_string_contains(&current, dir) {
        return Ok(());
    }
    let next = if current.trim().is_empty() {
        dir_text
    } else {
        format!("{current};{dir_text}")
    };
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "[Environment]::SetEnvironmentVariable('Path', $env:DSH_NEW_USER_PATH, 'User')",
    ])
    .env("DSH_NEW_USER_PATH", &next);
    hide_console(&mut cmd);
    let status = cmd
        .status()
        .map_err(|e| format!("无法写入用户 PATH: {e}"))?;
    if !status.success() {
        return Err(format!("写入用户 PATH 失败 (exit {status})"));
    }
    boot_log::info(&format!("user PATH appended {}", dir.display()));
    Ok(())
}

#[cfg(windows)]
fn read_windows_user_path() -> Result<String, String> {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "[Environment]::GetEnvironmentVariable('Path','User')",
    ]);
    hide_console(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("无法读取用户 PATH: {e}"))?;
    if !output.status.success() {
        return Err("读取用户 PATH 失败".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(not(windows))]
fn persist_unix_user_shim(bin_dir: &Path) -> Result<(), String> {
    let Some(home) = dirs::home_dir() else {
        return Ok(());
    };
    let local_bin = home.join(".local").join("bin");
    fs::create_dir_all(&local_bin).map_err(|e| e.to_string())?;
    let source = bin_dir.join("dsh");
    let dest = local_bin.join("dsh");
    if source.is_file() {
        fs::copy(&source, &dest).map_err(|e| e.to_string())?;
        set_executable(&dest)?;
    }
    if !path_string_contains(&std::env::var("PATH").unwrap_or_default(), &local_bin) {
        append_profile_path(&home, &local_bin)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn append_profile_path(home: &Path, local_bin: &Path) -> Result<(), String> {
    let profile = if cfg!(target_os = "macos") {
        home.join(".zprofile")
    } else {
        home.join(".profile")
    };
    let existing = fs::read_to_string(&profile).unwrap_or_default();
    if existing.contains(PROFILE_MARKER) {
        return Ok(());
    }
    let block = format!(
        "\n# {PROFILE_MARKER}\nexport PATH=\"{}:$PATH\"\n",
        local_bin.display()
    );
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&profile)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(block.as_bytes())
        })
        .map_err(|e| e.to_string())?;
    boot_log::info(&format!("profile PATH appended {}", profile.display()));
    Ok(())
}

#[cfg(not(windows))]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

fn quote_for_cmd(path: &Path) -> String {
    let mut text = path.display().to_string();
    #[cfg(windows)]
    {
        text = text.replace('/', "\\");
    }
    if text.contains(' ') || text.contains('&') {
        format!("\"{text}\"")
    } else {
        text
    }
}

fn push_unique_dir(dirs: &mut Vec<PathBuf>, candidate: Option<&Path>) {
    let Some(path) = candidate else {
        return;
    };
    if path.as_os_str().is_empty() || path_list_contains(dirs, path) {
        return;
    }
    dirs.push(path.to_path_buf());
}

fn push_env_join(dirs: &mut Vec<PathBuf>, key: &str, suffix: &[&str]) {
    if let Ok(root) = std::env::var(key) {
        if !root.trim().is_empty() {
            let mut path = PathBuf::from(root);
            for part in suffix {
                path.push(part);
            }
            dirs.push(path);
        }
    }
}

fn path_list_contains(dirs: &[PathBuf], candidate: &Path) -> bool {
    dirs.iter().any(|dir| path_eq(dir, candidate))
}

fn path_string_contains(path: &str, candidate: &Path) -> bool {
    std::env::split_paths(path).any(|dir| path_eq(&dir, candidate))
}


#[cfg(test)]
mod tests {
    use super::{
        merge_path, path_string_contains, quote_for_cmd, tool_exists_in, write_dsh_shims,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "dsh-desktop-path-{}-{}-{}",
            std::process::id(),
            nanos,
            id
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn prepends_missing_dirs_without_duplicating_existing_path() {
        let first = PathBuf::from("C:\\DeepSeek Harness\\bin");
        let second = PathBuf::from("C:\\DeepSeek Harness\\runtime\\node");
        let existing = std::env::join_paths([&second, &PathBuf::from("C:\\Windows\\System32")]).unwrap();
        let merged = merge_path(Some(existing), &[first.clone(), second.clone()]);
        let parts: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(parts[0], first);
        assert_eq!(
            parts
                .iter()
                .filter(|path| path_string_contains(&merged, path) && **path == second)
                .count(),
            1
        );
    }

    #[test]
    fn writes_dsh_shim_that_points_at_the_selected_node_and_cli() {
        let root = temp_root();
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let node = root.join("node.exe");
        let cli = root.join("apps").join("cli").join("lib").join("bin.js");
        write_dsh_shims(&bin, &node, &cli).unwrap();

        #[cfg(windows)]
        {
            let cmd = fs::read_to_string(bin.join("dsh.cmd")).unwrap();
            assert!(cmd.contains("node.exe"));
            assert!(cmd.contains("bin.js"));
            assert!(cmd.contains("%*"));
            let sh = fs::read_to_string(bin.join("dsh")).unwrap();
            assert!(sh.contains("exec"));
            assert!(sh.contains("\"$@\""));
        }
        #[cfg(not(windows))]
        {
            let sh = fs::read_to_string(bin.join("dsh")).unwrap();
            assert!(sh.starts_with("#!/bin/sh"));
            assert!(sh.contains("bin.js"));
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn quotes_paths_that_contain_spaces() {
        assert_eq!(
            quote_for_cmd(Path::new(r"C:\Program Files\nodejs\node.exe")),
            r#""C:\Program Files\nodejs\node.exe""#
        );
    }

    #[test]
    fn detects_a_tool_only_when_the_file_exists() {
        let root = temp_root();
        fs::write(root.join("git.exe"), "").unwrap();
        assert!(tool_exists_in(&root, "git"));
        assert!(!tool_exists_in(&root, "bash"));
        let _ = fs::remove_dir_all(&root);
    }
}
