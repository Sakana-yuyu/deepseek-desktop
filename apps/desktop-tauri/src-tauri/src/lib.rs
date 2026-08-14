mod chrome;
mod notify;
mod overlay;
mod runtime;
mod tray;
mod updater;
mod window_layout;

use runtime::boot_log;
use runtime::config::BUNDLED_HARNESS_DIR;
use runtime::provision::ensure_runtime;
use runtime::supervisor::HostOverlay;
use runtime::{DesktopRuntime, ProvisionEvent};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            chrome::show_main(app);
        }))
        .setup(|app| {
            let handle = app.handle().clone();
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("default window icon is missing")?;
            if let Some(splash) = app.get_webview_window("splash") {
                splash.set_icon(icon)?;
            }
            tray::install(&handle)?;
            let bundled = resolve_bundled_source(&handle);
            tauri::async_runtime::spawn(async move {
                if let Err(err) = boot_app(handle.clone(), bundled).await {
                    boot_log::error(&err);
                    let script = format!("window.__DSH_SPLASH__?.setError({});", json_string(&err));
                    let _ = splash_eval(&handle, &script);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn resolve_bundled_source(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join(BUNDLED_HARNESS_DIR))
        .filter(|path| path.join(".bundle-manifest.json").is_file())
}

async fn boot_app(app: AppHandle, bundled: Option<PathBuf>) -> Result<(), String> {
    boot_log::init()?;
    boot_log::info(&format!(
        "boot start bundled={}",
        bundled
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".into())
    ));

    let app_for_progress = app.clone();
    let progress: Arc<dyn Fn(ProvisionEvent) + Send + Sync> =
        Arc::new(move |event: ProvisionEvent| {
            let app = app_for_progress.clone();
            tauri::async_runtime::spawn(async move {
                let script = match event {
                    ProvisionEvent::Status(text) => {
                        boot_log::info(&format!("status: {text}"));
                        format!("window.__DSH_SPLASH__?.setStatus({});", json_string(&text))
                    }
                    ProvisionEvent::Progress(pct) => {
                        format!("window.__DSH_SPLASH__?.setProgress({pct});", pct = pct)
                    }
                };
                let _ = splash_eval(&app, &script);
            });
        });

    if let Err(error) = updater::install_available(&app, Arc::clone(&progress)).await {
        boot_log::error(&format!("desktop update skipped: {error}"));
        progress(ProvisionEvent::Status("更新检查失败，正在继续启动…".into()));
    }

    let notify = notify::start(app.clone())?;
    let paths = ensure_runtime(bundled, {
        let progress = Arc::clone(&progress);
        move |event| progress(event)
    })
    .await?;

    let overlay_src = overlay::resolve_overlay_source(app.path().resource_dir().ok().as_deref());
    let implanted = overlay::install_overlay(&paths, &overlay_src, &notify.url)?;
    let host_overlay = HostOverlay {
        patch_file: implanted.patch_file,
        notify_url: notify.url.clone(),
    };

    let runtime = DesktopRuntime::start(paths, Some(&host_overlay), progress).await?;
    let web_url = runtime.web_url.clone();
    app.manage(runtime);
    app.manage(notify);
    boot_log::info(&format!("opening main window url={web_url}"));
    chrome::open_main_window(&app, &web_url)?;
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
    boot_log::info("boot complete");
    Ok(())
}

fn splash_eval(app: &AppHandle, script: &str) -> Result<(), String> {
    if let Some(splash) = app.get_webview_window("splash") {
        splash.eval(script).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}
