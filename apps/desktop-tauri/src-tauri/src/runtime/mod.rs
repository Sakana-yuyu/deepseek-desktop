pub mod boot_log;
pub mod config;
pub mod env_path;
pub mod host_env;
pub mod io_fallback;
pub mod path_bridge;
pub mod profile_repair;
mod process;
pub mod provision;
pub mod supervisor;
pub mod user_home;
pub mod wsl;

use std::path::PathBuf;
use std::sync::Arc;

use crate::desktop_settings::{
    effective_agent_environment, AgentEnvironment, DesktopSettings,
};
use provision::RuntimePaths;
use supervisor::{HostHandle, HostOverlay};

/// Resolved Node + harness tree and the spawned Host child.
pub struct DesktopRuntime {
    pub paths: RuntimePaths,
    pub host: HostHandle,
    pub web_url: String,
}

impl DesktopRuntime {
    /// Start `dsh web` against an already provisioned Windows tree.
    ///
    /// Applies the Windows PATH bridge and profile repair, then spawns the
    /// Host with `node.exe`. WSL mode must use [`Self::start_wsl`] instead.
    pub async fn start(
        paths: RuntimePaths,
        overlay: Option<&HostOverlay>,
        progress: Arc<dyn Fn(ProvisionEvent) + Send + Sync>,
    ) -> Result<Self, String> {
        boot_log::info(&format!(
            "provision complete cli={} node={}",
            paths.cli_entry.display(),
            paths.node_binary.display()
        ));
        let host_path = match path_bridge::prepare_host_path(&paths, |event| progress(event)) {
            Ok(path) => path,
            Err(error) => {
                boot_log::info(&format!("path bridge fallback: {error}"));
                path_bridge::merge_path(Some(env_path::discovery_path()), &[])
            }
        };
        progress(ProvisionEvent::Status("正在检查 profile 依赖…".into()));
        if let Err(error) =
            profile_repair::ensure_profile_installs(&paths, &host_path, &progress).await
        {
            return Err(error);
        }
        progress(ProvisionEvent::Status("正在启动 Web 界面…".into()));
        let host = supervisor::spawn_web_host(&paths, overlay, &host_path).await?;
        boot_log::info(&format!("dsh web ready url={}", host.web_url));
        Ok(Self {
            paths,
            web_url: host.web_url.clone(),
            host,
        })
    }

    /// Wrap a Host already spawned inside WSL.
    ///
    /// Skips the Windows PATH bridge and Windows profile repair. `paths` is a
    /// documented placeholder: the live Linux tree lives on WSL runtime paths
    /// inside the supervisor session, not on Windows `RuntimePaths`.
    pub fn start_wsl(host: HostHandle) -> Self {
        boot_log::info(&format!("wsl dsh web ready url={}", host.web_url));
        Self {
            // Placeholder only — WSL Host does not consume Windows RuntimePaths.
            paths: RuntimePaths {
                node_binary: PathBuf::new(),
                pnpm_binary: PathBuf::new(),
                cli_entry: PathBuf::new(),
                harness_root: PathBuf::new(),
                runtime_root: PathBuf::new(),
                dsh_home: PathBuf::new(),
            },
            web_url: host.web_url.clone(),
            host,
        }
    }
}

/// Progress events for the splash UI.
#[derive(Clone, Debug)]
pub enum ProvisionEvent {
    Status(String),
    Progress(u8),
}

/// Which agent environment boot should enter, from persisted settings.
pub fn boot_kind(settings: &DesktopSettings) -> AgentEnvironment {
    effective_agent_environment(settings)
}

pub fn app_data_root() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|d| d.join("DeepSeek Harness"))
        .ok_or_else(|| "cannot resolve application data directory".into())
}

#[cfg(test)]
mod boot_kind_tests {
    use super::boot_kind;
    use crate::desktop_settings::{AgentEnvironment, DesktopSettings};

    #[test]
    fn default_settings_select_windows() {
        assert_eq!(boot_kind(&DesktopSettings::default()), AgentEnvironment::Windows);
    }

    #[test]
    fn wsl_settings_select_wsl() {
        assert_eq!(
            boot_kind(&DesktopSettings {
                agent_environment: AgentEnvironment::Wsl,
                ..DesktopSettings::default()
            }),
            AgentEnvironment::Wsl
        );
    }
}
