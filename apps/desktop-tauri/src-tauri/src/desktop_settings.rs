//! Persisted desktop-shell preferences next to `boot.log`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::app_data_root;

/// What the title-bar / window close button does after the user has chosen.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseAction {
    Minimize,
    Exit,
}

/// Where the desktop shell runs the agent runtime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentEnvironment {
    #[default]
    Windows,
    Wsl,
}

/// Desktop preferences stored as JSON under the application-data root.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
    #[serde(default)]
    pub close_action: Option<CloseAction>,
    #[serde(default)]
    pub agent_environment: AgentEnvironment,
    #[serde(default)]
    pub wsl_distro: Option<String>,
}

/// Path of `desktop-settings.json` beside `boot.log`.
pub fn settings_path() -> Result<PathBuf, String> {
    Ok(app_data_root()?.join("desktop-settings.json"))
}

/// Load preferences, or defaults when the file is missing or unreadable.
pub fn load() -> DesktopSettings {
    match settings_path() {
        Ok(path) => load_from(&path),
        Err(_) => DesktopSettings::default(),
    }
}

/// Persist preferences. Failure is logged by the caller.
pub fn save(settings: &DesktopSettings) -> Result<(), String> {
    save_to(&settings_path()?, settings)
}

/// Read one settings file. Invalid JSON becomes defaults.
pub fn load_from(path: &Path) -> DesktopSettings {
    let Ok(raw) = fs::read_to_string(path) else {
        return DesktopSettings::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Write one settings file, creating the parent directory.
pub fn save_to(path: &Path, settings: &DesktopSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 {}: {e}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, format!("{raw}\n")).map_err(|e| format!("无法写入 {}: {e}", path.display()))
}

/// Resolved agent runtime target from persisted settings.
pub fn effective_agent_environment(settings: &DesktopSettings) -> AgentEnvironment {
    settings.agent_environment
}

#[cfg(test)]
mod tests {
    use super::{load_from, save_to, AgentEnvironment, CloseAction, DesktopSettings};
    use std::fs;
    use std::path::PathBuf;

    fn temp_file() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "dsh-desktop-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&root);
        root.join("desktop-settings.json")
    }

    #[test]
    fn missing_file_is_unset_close_action() {
        let path = temp_file();
        let _ = fs::remove_file(&path);
        assert_eq!(load_from(&path).close_action, None);
    }

    #[test]
    fn omitted_agent_environment_is_windows() {
        let path = temp_file();
        let _ = fs::remove_file(&path);
        assert_eq!(
            load_from(&path).agent_environment,
            AgentEnvironment::Windows
        );
        assert_eq!(load_from(&path).wsl_distro, None);
    }

    #[test]
    fn persists_wsl_environment_and_distro() {
        let path = temp_file();
        save_to(
            &path,
            &DesktopSettings {
                close_action: None,
                agent_environment: AgentEnvironment::Wsl,
                wsl_distro: Some("Ubuntu".into()),
            },
        )
        .unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.agent_environment, AgentEnvironment::Wsl);
        assert_eq!(loaded.wsl_distro.as_deref(), Some("Ubuntu"));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("agentEnvironment"));
        assert!(raw.contains("wsl"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn persists_minimize_and_exit_close_actions() {
        let path = temp_file();
        save_to(
            &path,
            &DesktopSettings {
                close_action: Some(CloseAction::Minimize),
                ..DesktopSettings::default()
            },
        )
        .unwrap();
        assert_eq!(load_from(&path).close_action, Some(CloseAction::Minimize));
        save_to(
            &path,
            &DesktopSettings {
                close_action: Some(CloseAction::Exit),
                ..DesktopSettings::default()
            },
        )
        .unwrap();
        assert_eq!(load_from(&path).close_action, Some(CloseAction::Exit));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("closeAction"));
        save_to(
            &path,
            &DesktopSettings {
                close_action: None,
                ..DesktopSettings::default()
            },
        )
        .unwrap();
        assert_eq!(load_from(&path).close_action, None);
        let _ = fs::remove_file(&path);
    }
}
