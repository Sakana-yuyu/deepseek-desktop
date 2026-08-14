pub mod boot_log;
pub mod config;
mod process;
pub mod provision;
pub mod supervisor;

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
        progress(ProvisionEvent::Status("正在启动 Web 界面…".into()));
        let host = supervisor::spawn_web_host(&paths, overlay).await?;
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
