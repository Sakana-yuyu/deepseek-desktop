mod chrome;
mod cli_shim;
mod desktop_settings;
mod notify;
mod overlay;
mod runtime;
mod tray;
mod updater;
mod window_layout;

use runtime::boot_log;
use runtime::config::BUNDLED_HARNESS_DIR;
use runtime::io_fallback::is_recoverable_io;
use runtime::provision::{ensure_runtime, try_recover_paths};
use runtime::supervisor::HostOverlay;
use runtime::{DesktopRuntime, ProvisionEvent};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::window::Color;
use tauri::{AppHandle, Manager, RunEvent};

const SPLASH_BG: Color = Color(0, 0, 0, 0);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if cli_shim::should_run_as_cli() {
        std::process::exit(cli_shim::run());
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            chrome::show_main(app);
        }))
        .invoke_handler(tauri::generate_handler![chrome::set_close_action])
        .setup(|app| {
            let handle = app.handle().clone();
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("default window icon is missing")?;
            if let Some(splash) = app.get_webview_window("splash") {
                splash.set_icon(icon)?;
                let _ = splash.set_background_color(Some(SPLASH_BG));
                let _ = splash.center();
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                RunEvent::ExitRequested { api, .. } => {
                    if !chrome::quit_requested() {
                        api.prevent_exit();
                    } else {
                        chrome::stop_host(app);
                    }
                }
                RunEvent::Exit => chrome::stop_host(app),
                _ => {}
            }
        });
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

    let notify = match notify::start(app.clone()) {
        Ok(notify) => Some(notify),
        Err(error) => {
            boot_log::info(&format!("notify disabled, overlay skipped: {error}"));
            None
        }
    };
    let paths = match ensure_runtime(bundled.clone(), {
        let progress = Arc::clone(&progress);
        move |event| progress(event)
    })
    .await
    {
        Ok(paths) => paths,
        Err(error) => {
            boot_log::error(&format!("provision failed: {error}"));
            if let Some(paths) = try_recover_paths(bundled.as_deref()) {
                progress(ProvisionEvent::Status(
                    if is_recoverable_io(&error) {
                        "预配遇到占用或权限问题，改用已有运行时…".into()
                    } else {
                        "预配未完成，改用已有运行时…".into()
                    },
                ));
                paths
            } else if is_recoverable_io(&error) {
                return Err(
                    "启动遇到占用或权限问题，未能找到可用运行时。详见 boot.log。".into(),
                );
            } else {
                return Err(error);
            }
        }
    };

    let overlay_src = overlay::resolve_overlay_source(app.path().resource_dir().ok().as_deref());
    let host_overlay = notify.as_ref().and_then(|notify| {
        match overlay::install_overlay(&paths, &overlay_src, &notify.url) {
            Ok(implanted) => Some(HostOverlay {
                patch_file: implanted.patch_file,
                notify_url: notify.url.clone(),
            }),
            Err(error) => {
                boot_log::info(&format!("overlay skipped: {error}"));
                None
            }
        }
    });

    let runtime = DesktopRuntime::start(paths, host_overlay.as_ref(), progress).await?;
    let web_url = runtime.web_url.clone();
    if !runtime.host.disabled_plugins.is_empty() {
        let names = runtime.host.disabled_plugins.join("、");
        boot_log::error(&format!("plugins disabled by rescue patch: {names}"));
        notify::toast(
            &app,
            "DeepSeek Harness",
            &format!("以下插件已损坏，本次启动已自动禁用：{names}。修复或更新插件后重启即可恢复。"),
        );
    }
    app.manage(runtime);
    if let Some(notify) = notify {
        app.manage(notify);
    }
    boot_log::info(&format!("opening main window url={web_url}"));
    chrome::open_main_window(&app, &web_url)?;
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
    boot_log::info("boot complete");
    let app_for_update = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = updater::install_available(&app_for_update, Arc::new(|_| {})).await {
            boot_log::info(&format!("desktop update skipped: {error}"));
        }
    });
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
