use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::runtime::ProvisionEvent;

/// Install a signed desktop update before provisioning the bundled Harness tree.
pub async fn install_available(
    app: &AppHandle,
    progress: Arc<dyn Fn(ProvisionEvent) + Send + Sync>,
) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }

    progress(ProvisionEvent::Status("正在检查桌面更新…".into()));
    let Some(update) = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };

    progress(ProvisionEvent::Status(format!(
        "正在下载桌面更新 {}…",
        update.version
    )));
    let progress_for_download = Arc::clone(&progress);
    let mut downloaded = 0_u64;
    update
        .download_and_install(
            move |chunk_length, content_length| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                if let Some(content_length) = content_length.filter(|length| *length > 0) {
                    let percent =
                        ((downloaded.saturating_mul(100)) / content_length).min(100) as u8;
                    progress_for_download(ProvisionEvent::Progress(percent));
                }
            },
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;

    app.restart();
}

/// Check for a signed update from the tray. Debug builds skip the network.
pub async fn check_now(app: &AppHandle) -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Ok("开发构建不检查桌面更新".into());
    }

    let Some(update) = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok("当前已是最新版本".into());
    };

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    app.restart()
}
