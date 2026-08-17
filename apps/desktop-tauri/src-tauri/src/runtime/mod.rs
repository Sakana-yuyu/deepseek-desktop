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

use provision::RuntimePaths;
use supervisor::{HostHandle, HostOverlay};

/// Resolved Node + harness tree and the spawned Host child.
pub struct DesktopRuntime {
    pub paths: RuntimePaths,
    pub host: HostHandle,
    pub web_url: String,
}

impl DesktopRuntime {
    /// Start `dsh web` against an already provisioned tree.
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
}

/// Progress events for the splash UI.
#[derive(Clone, Debug)]
pub enum ProvisionEvent {
    Status(String),
    Progress(u8),
}

pub fn app_data_root() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|d| d.join("DeepSeek Harness"))
        .ok_or_else(|| "cannot resolve application data directory".into())
}
