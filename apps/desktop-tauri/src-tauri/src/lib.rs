mod runtime;

use runtime::boot_log;
use runtime::config::BUNDLED_HARNESS_DIR;
use runtime::{DesktopRuntime, ProvisionEvent};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("default window icon is missing")?;
            app.get_webview_window("splash")
                .ok_or("splash window is missing")?
                .set_icon(icon)?;
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
    let progress = Arc::new(move |event: ProvisionEvent| {
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

    let runtime = DesktopRuntime::boot(bundled, progress).await?;
    let web_url = runtime.web_url.clone();
    // Keep the child process alive for the app lifetime; dropping `DesktopRuntime`
    // would kill `dsh web` via `HostHandle::drop`.
    app.manage(runtime);
    boot_log::info(&format!("opening main window url={web_url}"));
    open_main_window(&app, &web_url)?;
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
    boot_log::info("boot complete");
    Ok(())
}

fn open_main_window(app: &AppHandle, url: &str) -> Result<(), String> {
    if app.get_webview_window("main").is_some() {
        return Ok(());
    }

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default window icon is missing".to_string())?;
    let builder = WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External(url.parse().map_err(|e| format!("invalid web url: {e}"))?),
    )
    .title("DeepSeek Harness")
    .inner_size(1280.0, 860.0)
    .center()
    .visible(false);
    let window = builder
        .icon(icon)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;

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
