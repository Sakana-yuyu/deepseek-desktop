//! Put the Host's required CLIs on PATH: process env first, user PATH when missing.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::cli_shim::{DshLaunchSpec, LAUNCH_FILE};
use crate::i18n::{self, Msg};

use super::boot_log;
use super::env_path::{discovery_path, path_eq, which_on_host};
use super::io_fallback::recoverable_message;

#[cfg(windows)]
use super::env_path::is_direct_spawnable_cli;
use super::provision::RuntimePaths;
use super::{app_data_root, ProvisionEvent};

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

/// Write `dsh` / `pnpm` shims and return a PATH that includes every required CLI directory.
pub fn prepare_host_path(
    paths: &RuntimePaths,
    progress: impl Fn(ProvisionEvent),
) -> Result<String, String> {
    progress(ProvisionEvent::Status(i18n::t(Msg::StatusWritePath).into()));
    let bridge = match install_path_bridge(paths) {
        Ok(bridge) => bridge,
        Err(error) => {
            boot_log::info(&format!("path bridge install fallback: {error}"));
            return Ok(merge_path(Some(discovery_path()), &[]));
        }
    };
    if let Err(error) = persist_user_path(&bridge) {
        boot_log::info(&format!("user PATH persist skipped: {error}"));
    }
    let merged = merge_path(Some(discovery_path()), &bridge.prepend);
    std::env::set_var("PATH", &merged);
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
    fs::create_dir_all(&bin_dir).map_err(|e| recoverable_message("create", &bin_dir, e))?;
    write_cli_shims(
        &bin_dir,
        &paths.node_binary,
        &paths.cli_entry,
        &paths.pnpm_binary,
        &paths.dsh_home,
        true,
    )?;

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

/// Write Host-facing `dsh` / `pnpm` shims. `install_exe` copies the desktop binary as `dsh.exe`.
pub fn write_cli_shims(
    bin_dir: &Path,
    node: &Path,
    cli_entry: &Path,
    pnpm: &Path,
    dsh_home: &Path,
    install_exe: bool,
) -> Result<(), String> {
    let path_prepend = shim_path_prepend(node, pnpm);
    write_dsh_cmd_shim(bin_dir, node, cli_entry, &path_prepend)?;
    write_launch_spec(bin_dir, node, cli_entry, dsh_home, &path_prepend)?;
    write_pnpm_shim(bin_dir, node, pnpm)?;
    if install_exe {
        install_dsh_exe(bin_dir)?;
    }
    Ok(())
}

/// Directories every `dsh` shim prepends to `PATH`: the provisioned Node and
/// pnpm directories, so a terminal `dsh plugin` uses the same pnpm as the
/// desktop instead of splitting one profile across two pnpm-major stores.
fn shim_path_prepend(node: &Path, pnpm: &Path) -> Vec<PathBuf> {
    let mut prepend = Vec::new();
    push_unique_dir(&mut prepend, node.parent());
    push_unique_dir(&mut prepend, pnpm.parent());
    prepend
}

fn write_dsh_cmd_shim(
    bin_dir: &Path,
    node: &Path,
    cli_entry: &Path,
    path_prepend: &[PathBuf],
) -> Result<(), String> {
    let node_q = quote_for_cmd(node);
    let cli_q = quote_for_cmd(cli_entry);
    let prepend = std::env::join_paths(path_prepend)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();

    #[cfg(windows)]
    {
        // An extensionless `dsh` on Windows shadows `dsh.cmd` for Node spawn('dsh').
        let _ = fs::remove_file(bin_dir.join("dsh"));
        let dest = bin_dir.join("dsh.cmd");
        fs::write(
            &dest,
            format!("@echo off\r\nset \"PATH={prepend};%PATH%\"\r\n{node_q} {cli_q} %*\r\n"),
        )
        .map_err(|e| recoverable_message("write", &dest, e))?;
    }

    #[cfg(not(windows))]
    {
        let script = bin_dir.join("dsh");
        fs::write(
            &script,
            format!("#!/bin/sh\nPATH=\"{prepend}:$PATH\"\nexec {node_q} {cli_q} \"$@\"\n"),
        )
        .map_err(|e| recoverable_message("write", &script, e))?;
        set_executable(&script)?;
    }

    Ok(())
}

fn write_launch_spec(
    bin_dir: &Path,
    node: &Path,
    cli_entry: &Path,
    dsh_home: &Path,
    path_prepend: &[PathBuf],
) -> Result<(), String> {
    let spec = DshLaunchSpec {
        node: node.display().to_string(),
        cli: cli_entry.display().to_string(),
        dsh_home: dsh_home.display().to_string(),
        path_prepend: path_prepend
            .iter()
            .map(|dir| dir.display().to_string())
            .collect(),
    };
    let raw = serde_json::to_string_pretty(&spec).map_err(|e| e.to_string())?;
    let dest = bin_dir.join(LAUNCH_FILE);
    fs::write(&dest, format!("{raw}\n")).map_err(|e| recoverable_message("write", &dest, e))
}

fn write_pnpm_shim(bin_dir: &Path, node: &Path, pnpm: &Path) -> Result<(), String> {
    let body = pnpm_shim_body(node, pnpm);

    #[cfg(windows)]
    {
        let dest = bin_dir.join("pnpm.cmd");
        fs::write(&dest, body).map_err(|e| recoverable_message("write", &dest, e))?;
    }

    #[cfg(not(windows))]
    {
        let script = bin_dir.join("pnpm");
        fs::write(&script, body).map_err(|e| recoverable_message("write", &script, e))?;
        set_executable(&script)?;
    }

    Ok(())
}

fn pnpm_shim_body(node: &Path, pnpm: &Path) -> String {
    if let Some(cjs) = find_pnpm_cjs(pnpm) {
        #[cfg(windows)]
        {
            return format!(
                "@echo off\r\n{} {} %*\r\n",
                quote_for_cmd(node),
                quote_for_cmd(&cjs)
            );
        }
        #[cfg(not(windows))]
        {
            return format!(
                "#!/bin/sh\nexec {} {} \"$@\"\n",
                quote_for_cmd(node),
                quote_for_cmd(&cjs)
            );
        }
    }

    #[cfg(windows)]
    {
        if pnpm
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        {
            return format!("@echo off\r\ncall {} %*\r\n", quote_for_cmd(pnpm));
        }
        format!("@echo off\r\n{} %*\r\n", quote_for_cmd(pnpm))
    }

    #[cfg(not(windows))]
    {
        format!("#!/bin/sh\nexec {} \"$@\"\n", quote_for_cmd(pnpm))
    }
}

fn find_pnpm_cjs(pnpm: &Path) -> Option<PathBuf> {
    let dir = pnpm.parent()?;
    let candidates = [
        dir.join("node_modules")
            .join("pnpm")
            .join("bin")
            .join("pnpm.cjs"),
        dir.join("lib")
            .join("node_modules")
            .join("pnpm")
            .join("bin")
            .join("pnpm.cjs"),
        dir.join("pnpm").join("bin").join("pnpm.cjs"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn install_dsh_exe(bin_dir: &Path) -> Result<(), String> {
    let Ok(source) = std::env::current_exe() else {
        return Ok(());
    };
    if source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("dsh"))
    {
        return Ok(());
    }
    let dest = bin_dir.join("dsh.exe");
    if same_exe(&source, &dest) || !exe_needs_refresh(&source, &dest) {
        return Ok(());
    }
    match fs::remove_file(&dest) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            boot_log::info(&format!(
                "dsh.exe refresh skipped ({}): {error}",
                dest.display()
            ));
            return Ok(());
        }
    }
    if fs::hard_link(&source, &dest).is_ok() {
        boot_log::info(&format!("dsh.exe hard-linked from {}", source.display()));
        return Ok(());
    }
    fs::copy(&source, &dest).map_err(|e| format!("无法写入 {}: {e}", dest.display()))?;
    boot_log::info(&format!("dsh.exe copied from {}", source.display()));
    Ok(())
}

fn same_exe(left: &Path, right: &Path) -> bool {
    if !left.is_file() || !right.is_file() {
        return false;
    }
    if path_eq(left, right) {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => path_eq(&a, &b),
        _ => false,
    }
}

/// True when `dest` is missing, a different size, or older than `source`.
fn exe_needs_refresh(source: &Path, dest: &Path) -> bool {
    let Ok(src) = fs::metadata(source) else {
        return false;
    };
    let Ok(dst) = fs::metadata(dest) else {
        return true;
    };
    if src.len() != dst.len() {
        return true;
    }
    match (src.modified(), dst.modified()) {
        (Ok(src_mtime), Ok(dst_mtime)) => src_mtime > dst_mtime,
        _ => false,
    }
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
fn persist_windows_user_path_if_missing(dir: Option<&Path>, names: &[&str]) -> Result<(), String> {
    let Some(dir) = dir else {
        return Ok(());
    };
    if names.iter().any(|name| host_has_spawnable(name)) {
        return Ok(());
    }
    persist_windows_user_path(dir)
}

#[cfg(windows)]
fn host_has_spawnable(name: &str) -> bool {
    which_on_host(name)
        .map(|path| is_direct_spawnable_cli(&path))
        .unwrap_or(false)
}

#[cfg(windows)]
fn persist_windows_user_path(dir: &Path) -> Result<(), String> {
    let current = read_windows_user_path()?;
    if path_string_contains(&current, dir) {
        return Ok(());
    }
    let dir_text = dir.display().to_string();
    let next = if current.trim().is_empty() {
        dir_text
    } else {
        format!("{current};{dir_text}")
    };
    write_windows_user_path(&next)?;
    broadcast_environment_change();
    boot_log::info(&format!("user PATH appended {}", dir.display()));
    Ok(())
}

#[cfg(windows)]
fn read_windows_user_path() -> Result<String, String> {
    let env = open_user_environment(false)?;
    match env.get_value::<String, _>("Path") {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("无法读取用户 PATH: {error}")),
    }
}

#[cfg(windows)]
fn write_windows_user_path(value: &str) -> Result<(), String> {
    use winreg::enums::RegType::REG_EXPAND_SZ;
    use winreg::types::ToRegValue;
    let env = open_user_environment(true)?;
    let mut raw = value.to_reg_value();
    raw.vtype = REG_EXPAND_SZ;
    env.set_raw_value("Path", &raw)
        .map_err(|e| format!("无法写入用户 PATH: {e}"))
}

#[cfg(windows)]
fn open_user_environment(write: bool) -> Result<winreg::RegKey, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    let flags = if write {
        KEY_READ | KEY_WRITE
    } else {
        KEY_READ
    };
    hkcu.open_subkey_with_flags("Environment", flags)
        .map_err(|e| format!("无法打开 HKCU\\Environment: {e}"))
}

#[cfg(windows)]
fn broadcast_environment_change() {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }
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
        exe_needs_refresh, merge_path, path_string_contains, pnpm_shim_body, quote_for_cmd,
        tool_exists_in, write_cli_shims,
    };
    use crate::cli_shim::{read_launch_spec, LAUNCH_FILE};
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
        let existing =
            std::env::join_paths([&second, &PathBuf::from("C:\\Windows\\System32")]).unwrap();
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
    fn writes_spawnable_dsh_and_pnpm_shims() {
        let root = temp_root();
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("dsh"), "#!/bin/sh\nold\n").unwrap();
        let node = root.join("node.exe");
        let cli = root.join("apps").join("cli").join("lib").join("bin.js");
        let pnpm_home = root.join("pnpm-global");
        let pnpm_cjs = pnpm_home
            .join("node_modules")
            .join("pnpm")
            .join("bin")
            .join("pnpm.cjs");
        fs::create_dir_all(pnpm_cjs.parent().unwrap()).unwrap();
        fs::write(&pnpm_cjs, "module.exports = {}\n").unwrap();
        let pnpm = pnpm_home.join("pnpm.cmd");
        let home = root.join("dsh-home");
        write_cli_shims(&bin, &node, &cli, &pnpm, &home, false).unwrap();

        #[cfg(windows)]
        {
            assert!(!bin.join("dsh").is_file());
            let cmd = fs::read_to_string(bin.join("dsh.cmd")).unwrap();
            assert!(cmd.contains("node.exe"));
            assert!(cmd.contains("bin.js"));
            assert!(cmd.contains("%*"));
            assert!(cmd.contains("pnpm-global"));
            let pnpm_cmd = fs::read_to_string(bin.join("pnpm.cmd")).unwrap();
            assert!(pnpm_cmd.contains("node.exe"));
            assert!(pnpm_cmd.contains("pnpm.cjs"));
            let spec = read_launch_spec(&bin.join(LAUNCH_FILE)).unwrap();
            assert!(spec.cli.contains("bin.js"));
            assert!(spec.dsh_home.contains("dsh-home"));
            let expected_prepend: Vec<String> = [node.parent().unwrap(), pnpm.parent().unwrap()]
                .iter()
                .map(|dir| dir.display().to_string())
                .collect();
            assert_eq!(spec.path_prepend, expected_prepend);
        }
        #[cfg(not(windows))]
        {
            let sh = fs::read_to_string(bin.join("dsh")).unwrap();
            assert!(sh.starts_with("#!/bin/sh"));
            assert!(sh.contains("bin.js"));
            assert!(sh.contains("pnpm-global"));
            let pnpm_sh = fs::read_to_string(bin.join("pnpm")).unwrap();
            assert!(pnpm_sh.contains("pnpm.cjs"));
            let spec = read_launch_spec(&bin.join(LAUNCH_FILE)).unwrap();
            let expected_prepend: Vec<String> = [node.parent().unwrap(), pnpm.parent().unwrap()]
                .iter()
                .map(|dir| dir.display().to_string())
                .collect();
            assert_eq!(spec.path_prepend, expected_prepend);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pnpm_shim_falls_back_to_calling_a_cmd_when_cjs_is_missing() {
        let body = pnpm_shim_body(
            Path::new(r"C:\Program Files\nodejs\node.exe"),
            Path::new(r"C:\Users\me\AppData\Roaming\npm\pnpm.cmd"),
        );
        #[cfg(windows)]
        {
            assert!(body.contains("call"));
            assert!(body.contains("pnpm.cmd"));
        }
        #[cfg(not(windows))]
        {
            assert!(body.contains("exec"));
            assert!(body.contains("pnpm.cmd"));
        }
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

    #[test]
    fn skips_exe_refresh_when_dest_matches_size_and_is_not_older() {
        let root = temp_root();
        let source = root.join("dsh-desktop.exe");
        let dest = root.join("dsh.exe");
        fs::write(&source, b"desktop-binary").unwrap();
        fs::copy(&source, &dest).unwrap();
        assert!(!exe_needs_refresh(&source, &dest));
        fs::write(&dest, b"stale").unwrap();
        assert!(exe_needs_refresh(&source, &dest));
        let _ = fs::remove_dir_all(&root);
    }
}
