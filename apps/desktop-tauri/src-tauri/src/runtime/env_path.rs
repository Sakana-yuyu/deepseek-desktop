//! Read the durable user environment, not only the GUI process PATH.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
use super::process::hide_console;

/// PATH used to discover host tools: the process PATH plus durable extras.
pub fn discovery_path() -> OsString {
    append_unique_path(std::env::var_os("PATH"), &durable_path_dirs())
}

/// Resolve `name` on {@link discovery_path}, including Windows `.cmd` / `.exe`.
pub fn which_on_host(name: &str) -> Option<PathBuf> {
    let path = discovery_path();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Ok(found) = which::which_in(name, Some(&path), &cwd) {
        return Some(found);
    }
    #[cfg(windows)]
    if !name.contains('.') {
        for suffix in [".cmd", ".exe"] {
            if let Ok(found) = which::which_in(format!("{name}{suffix}"), Some(&path), &cwd) {
                return Some(found);
            }
        }
    }
    None
}

/// `$DSH_HOME` from the process, then the durable user/machine environment.
pub fn durable_dsh_home() -> Option<PathBuf> {
    first_nonempty([
        std::env::var("DSH_HOME").ok(),
        #[cfg(windows)]
        read_windows_env("DSH_HOME", "User"),
        #[cfg(windows)]
        read_windows_env("DSH_HOME", "Machine"),
    ])
    .map(|raw| expand_home_prefix(&raw))
}

/// True when two filesystem paths name the same location for PATH / home matching.
pub fn path_eq(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_path(left).eq_ignore_ascii_case(&normalize_windows_path(right))
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Append directories that are not already on `existing`, preserving order.
pub fn append_unique_path(existing: Option<impl AsRef<std::ffi::OsStr>>, extra: &[PathBuf]) -> OsString {
    let mut parts = Vec::new();
    if let Some(existing) = existing {
        for dir in std::env::split_paths(existing.as_ref()) {
            if !dir.as_os_str().is_empty() && !path_list_contains(&parts, &dir) {
                parts.push(dir);
            }
        }
    }
    for dir in extra {
        if !dir.as_os_str().is_empty() && !path_list_contains(&parts, dir) {
            parts.push(dir.clone());
        }
    }
    std::env::join_paths(parts).unwrap_or_else(|_| extra.first().cloned().unwrap_or_default().into())
}

fn durable_path_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut dirs = Vec::new();
        push_path_list(&mut dirs, read_windows_env("Path", "User"));
        push_path_list(&mut dirs, read_windows_env("Path", "Machine"));
        dirs
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn push_path_list(dirs: &mut Vec<PathBuf>, raw: Option<String>) {
    let Some(raw) = raw else {
        return;
    };
    for dir in std::env::split_paths(&raw) {
        if !dir.as_os_str().is_empty() && !path_list_contains(dirs, &dir) {
            dirs.push(dir);
        }
    }
}

fn path_list_contains(dirs: &[PathBuf], candidate: &Path) -> bool {
    dirs.iter().any(|dir| path_eq(dir, candidate))
}

fn first_nonempty<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values.into_iter().flatten().find(|value| !value.trim().is_empty())
}

fn expand_home_prefix(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

#[cfg(windows)]
fn normalize_windows_path(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('/', "\\");
    while text.len() > 3 && text.ends_with('\\') {
        text.pop();
    }
    text
}

#[cfg(windows)]
fn read_windows_env(name: &str, scope: &str) -> Option<String> {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        &format!("[Environment]::GetEnvironmentVariable('{name}','{scope}')"),
    ]);
    hide_console(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{append_unique_path, path_eq};
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn appends_missing_dirs_after_the_existing_path() {
        let existing = std::env::join_paths([
            PathBuf::from(r"C:\Windows\System32"),
            PathBuf::from(r"C:\Program Files\nodejs"),
        ])
        .unwrap();
        let extra = [
            PathBuf::from(r"C:\Program Files\nodejs\"),
            PathBuf::from(r"C:\Users\me\AppData\Roaming\npm"),
        ];
        let merged = append_unique_path(Some(existing), &extra);
        let parts: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(parts[0], PathBuf::from(r"C:\Windows\System32"));
        assert_eq!(
            parts
                .iter()
                .filter(|path| path_eq(path, &PathBuf::from(r"C:\Program Files\nodejs")))
                .count(),
            1
        );
        assert!(parts
            .iter()
            .any(|path| path_eq(path, &PathBuf::from(r"C:\Users\me\AppData\Roaming\npm"))));
    }

    #[cfg(windows)]
    #[test]
    fn treats_windows_paths_as_equal_across_slash_and_case() {
        assert!(path_eq(
            &PathBuf::from(r"C:\Users\Me\.dsh"),
            &PathBuf::from(r"c:/users/me/.dsh/"),
        ));
        assert!(!path_eq(
            &PathBuf::from(r"C:\Users\Me\.dsh"),
            &PathBuf::from(r"C:\Users\Me\dsh-home"),
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn appends_missing_unix_dirs_without_duplicates() {
        let existing = std::env::join_paths([
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .unwrap();
        let extra = [
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/home/me/.local/share/pnpm"),
        ];
        let merged = append_unique_path(Some(existing), &extra);
        let parts: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(parts[0], PathBuf::from("/usr/bin"));
        assert_eq!(
            parts
                .iter()
                .filter(|path| path_eq(path, &PathBuf::from("/usr/local/bin")))
                .count(),
            1
        );
        assert!(parts
            .iter()
            .any(|path| path_eq(path, &PathBuf::from("/home/me/.local/share/pnpm"))));
    }
}
