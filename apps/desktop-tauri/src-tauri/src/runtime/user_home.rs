//! Adopt an existing Harness home so CLI sessions and keys reach the desktop Host.

use std::fs;
use std::path::{Path, PathBuf};

use super::boot_log;
use super::env_path::{durable_dsh_home, path_eq};

const HOME_DIR_NAME: &str = ".dsh";
const SKIP_IMPORT: &[&str] = &["desktop-overlay"];
const HOME_MARKERS: &[&str] = &[
    "sessions",
    ".credentials.yaml",
    ".env",
    "profiles",
    "settings.yaml",
    "settings.yml",
    "settings.json",
];

/// Selected Host home plus how many missing entries were imported into it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUserHome {
    pub path: PathBuf,
    pub imported: usize,
}

/// Pick `$DSH_HOME` or `~/.dsh` when they already hold Harness data, then
/// copy missing sessions, credentials, and settings from every other home.
pub fn resolve_user_home(isolated: &Path) -> Result<ResolvedUserHome, String> {
    adopt_homes(isolated, discover_harness_homes(isolated))
}

fn adopt_homes(isolated: &Path, homes: Vec<PathBuf>) -> Result<ResolvedUserHome, String> {
    let selected = homes
        .first()
        .cloned()
        .unwrap_or_else(|| isolated.to_path_buf());
    fs::create_dir_all(&selected).map_err(|e| e.to_string())?;

    let mut imported = 0usize;
    for source in &homes {
        if path_eq(source, &selected) {
            continue;
        }
        imported += import_missing(source, &selected)?;
    }

    boot_log::info(&format!(
        "dsh home selected={} imported={imported} candidates={}",
        selected.display(),
        homes.len()
    ));
    Ok(ResolvedUserHome {
        path: selected,
        imported,
    })
}

/// Splash line after home matching.
pub fn user_home_status(resolved: &ResolvedUserHome, isolated: &Path) -> String {
    if resolved.path == isolated && resolved.imported == 0 {
        "未发现已有对话，将使用桌面端主目录".into()
    } else if resolved.imported == 0 {
        format!("已匹配已有主目录 {}", display_home(&resolved.path))
    } else {
        format!(
            "已恢复 {} 项历史数据到 {}",
            resolved.imported,
            display_home(&resolved.path)
        )
    }
}

/// True when `path` looks like a Harness home rather than an empty folder.
pub fn is_harness_home(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    HOME_MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Copy `from` into `to` without replacing files that already exist.
pub fn import_missing(from: &Path, to: &Path) -> Result<usize, String> {
    if !from.is_dir() || path_eq(from, to) {
        return Ok(0);
    }
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut copied = 0usize;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if SKIP_IMPORT.iter().any(|skip| name == *skip) {
            continue;
        }
        copied += import_entry(&entry.path(), &to.join(name))?;
    }
    Ok(copied)
}

fn discover_harness_homes(isolated: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    push_unique(&mut homes, env_dsh_home());
    push_unique(&mut homes, default_cli_home());
    if is_harness_home(isolated) {
        push_unique(&mut homes, Some(isolated.to_path_buf()));
    }
    homes
        .into_iter()
        .filter(|path| is_harness_home(path))
        .collect()
}

fn env_dsh_home() -> Option<PathBuf> {
    durable_dsh_home()
}

fn default_cli_home() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(HOME_DIR_NAME))
}

fn push_unique(homes: &mut Vec<PathBuf>, candidate: Option<PathBuf>) {
    let Some(path) = candidate else {
        return;
    };
    if !homes.iter().any(|existing| path_eq(existing, &path)) {
        homes.push(path);
    }
}

fn import_entry(from: &Path, to: &Path) -> Result<usize, String> {
    if from.is_dir() {
        if to.is_file() {
            return Ok(0);
        }
        if !to.exists() {
            copy_tree(from, to)?;
            return Ok(1);
        }
        let mut copied = 0usize;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copied += import_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(copied);
    }

    if to.exists() {
        return Ok(0);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(from, to).map_err(|e| e.to_string())?;
    Ok(1)
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
        copy_tree(&entry.path(), &dest.join(entry.file_name()))?;
    }
    Ok(())
}

fn display_home(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if path_eq(path, &home.join(HOME_DIR_NAME)) {
            return format!("~/{HOME_DIR_NAME}");
        }
    }
    if let Some(configured) = durable_dsh_home() {
        if path_eq(path, &configured) {
            return "$DSH_HOME".into();
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{adopt_homes, import_missing, is_harness_home, user_home_status};
    use std::fs;
    use std::path::PathBuf;
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
            "dsh-desktop-home-{}-{}-{}",
            std::process::id(),
            nanos,
            id
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn empty_directory_is_not_a_harness_home() {
        let root = temp_root();
        assert!(!is_harness_home(&root));
        fs::write(root.join(".credentials.yaml"), "DEEPSEEK_API_KEY: sk-test\n").unwrap();
        assert!(is_harness_home(&root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_missing_sessions_and_keys_without_overwriting() {
        let root = temp_root();
        let from = root.join("cli");
        let to = root.join("desktop");
        fs::create_dir_all(from.join("sessions").join("old")).unwrap();
        fs::write(from.join("sessions").join("old").join("log.jsonl"), "cli\n").unwrap();
        fs::write(from.join(".credentials.yaml"), "from: cli\n").unwrap();
        fs::write(from.join(".env"), "DEEPSEEK_API_KEY=cli\n").unwrap();
        fs::create_dir_all(to.join("sessions").join("old")).unwrap();
        fs::write(to.join("sessions").join("old").join("log.jsonl"), "desktop\n").unwrap();
        fs::write(to.join(".credentials.yaml"), "from: desktop\n").unwrap();

        let copied = import_missing(&from, &to).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            fs::read_to_string(to.join("sessions").join("old").join("log.jsonl")).unwrap(),
            "desktop\n"
        );
        assert_eq!(
            fs::read_to_string(to.join(".credentials.yaml")).unwrap(),
            "from: desktop\n"
        );
        assert_eq!(
            fs::read_to_string(to.join(".env")).unwrap(),
            "DEEPSEEK_API_KEY=cli\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_desktop_overlay_when_importing() {
        let root = temp_root();
        let from = root.join("cli");
        let to = root.join("desktop");
        fs::create_dir_all(from.join("desktop-overlay")).unwrap();
        fs::write(from.join("desktop-overlay").join("index.mjs"), "stolen\n").unwrap();
        fs::write(from.join(".env"), "DEEPSEEK_API_KEY=cli\n").unwrap();
        fs::create_dir_all(&to).unwrap();

        assert_eq!(import_missing(&from, &to).unwrap(), 1);
        assert!(!to.join("desktop-overlay").exists());
        assert!(to.join(".env").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn isolated_home_stays_selected_when_no_cli_home_exists() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        fs::create_dir_all(&isolated).unwrap();
        let resolved = adopt_homes(&isolated, Vec::new()).unwrap();
        assert_eq!(resolved.path, isolated);
        assert_eq!(resolved.imported, 0);
        assert!(user_home_status(&resolved, &isolated).contains("未发现已有对话"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prefers_cli_home_and_imports_desktop_only_sessions() {
        let root = temp_root();
        let cli = root.join("cli-home");
        let isolated = root.join("dsh-home");
        fs::create_dir_all(cli.join("sessions").join("from-cli")).unwrap();
        fs::write(cli.join(".credentials.yaml"), "DEEPSEEK_API_KEY: cli\n").unwrap();
        fs::create_dir_all(isolated.join("sessions").join("from-desktop")).unwrap();
        fs::write(
            isolated.join("sessions").join("from-desktop").join("log.jsonl"),
            "desktop\n",
        )
        .unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone(), isolated.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(resolved.imported >= 1);
        assert!(cli.join("sessions").join("from-desktop").is_dir());
        assert_eq!(
            fs::read_to_string(cli.join(".credentials.yaml")).unwrap(),
            "DEEPSEEK_API_KEY: cli\n"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
