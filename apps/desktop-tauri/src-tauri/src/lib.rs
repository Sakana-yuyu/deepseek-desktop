mod chrome;
mod cli_shim;
mod desktop_settings;
mod i18n;
mod notify;
mod overlay;
mod runtime;
mod tray;
mod updater;
mod window_layout;

use desktop_settings::AgentEnvironment;
use i18n::Msg;
use runtime::boot_log;
use runtime::config::BUNDLED_HARNESS_DIR;
use runtime::io_fallback::is_recoverable_io;
use runtime::provision::{ensure_runtime, read_bundle_hash, try_recover_paths};
use runtime::supervisor::{spawn_wsl_web_host, HostOverlay};
use runtime::user_home::resolve_user_home;
use runtime::wsl::{
    ensure_wsl_runtime, parse_wsl_list, select_distro, SystemWslRunner, WslRunner, WslSelectError,
};
use runtime::{app_data_root, boot_kind, DesktopRuntime, ProvisionEvent};
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
        .invoke_handler(tauri::generate_handler![
            chrome::set_close_action,
            chrome::restart_app
        ])
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
                let locale = match i18n::current() {
                    i18n::Locale::Zh => "zh",
                    i18n::Locale::En => "en",
                };
                let _ = splash.eval(&format!(
                    "window.__DSH_LOCALE__={};window.DSH_I18N&&window.DSH_I18N.apply();",
                    json_string(locale)
                ));
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
        .run(|app, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                if !chrome::quit_requested() {
                    api.prevent_exit();
                } else {
                    chrome::stop_host(app);
                }
            }
            RunEvent::Exit => chrome::stop_host(app),
            _ => {}
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

    let settings = desktop_settings::load();
    let runtime = match boot_kind(&settings) {
        AgentEnvironment::Windows => {
            boot_windows_runtime(app.clone(), bundled, notify.as_ref(), Arc::clone(&progress))
                .await?
        }
        AgentEnvironment::Wsl => {
            boot_wsl_runtime(
                app.clone(),
                bundled,
                &settings,
                notify.as_ref(),
                Arc::clone(&progress),
            )
            .await?
        }
    };

    let web_url = runtime.web_url.clone();
    if !runtime.host.disabled_plugins.is_empty() {
        let names = runtime.host.disabled_plugins.join("、");
        boot_log::error(&format!("plugins disabled by rescue patch: {names}"));
        notify::toast(
            &app,
            "DeepSeek Harness",
            &i18n::tf(Msg::PluginsDisabled, &names),
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

async fn boot_windows_runtime(
    app: AppHandle,
    bundled: Option<PathBuf>,
    notify: Option<&notify::NotifyHandle>,
    progress: Arc<dyn Fn(ProvisionEvent) + Send + Sync>,
) -> Result<DesktopRuntime, String> {
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
                progress(ProvisionEvent::Status(if is_recoverable_io(&error) {
                    i18n::t(Msg::BootRecoverIo).into()
                } else {
                    i18n::t(Msg::BootRecoverGeneric).into()
                }));
                paths
            } else if is_recoverable_io(&error) {
                return Err(i18n::t(Msg::BootRecoverFailed).into());
            } else {
                return Err(error);
            }
        }
    };

    let overlay_src = overlay::resolve_overlay_source(app.path().resource_dir().ok().as_deref());
    let host_overlay = notify.and_then(|notify| {
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

    DesktopRuntime::start(paths, host_overlay.as_ref(), progress).await
}

async fn boot_wsl_runtime(
    app: AppHandle,
    bundled: Option<PathBuf>,
    settings: &desktop_settings::DesktopSettings,
    notify: Option<&notify::NotifyHandle>,
    progress: Arc<dyn Fn(ProvisionEvent) + Send + Sync>,
) -> Result<DesktopRuntime, String> {
    progress(ProvisionEvent::Status(i18n::t(Msg::StatusDetectWsl).into()));
    let runner = SystemWslRunner;
    let list_out = runner
        .run(&["-l", "-v"])
        .map_err(|_| WslSelectError::missing_wsl().splash_message().to_string())?;
    let list_text = String::from_utf8_lossy(&list_out.stdout);
    let distros = parse_wsl_list(&list_text);
    let distro = select_distro(&distros, settings.wsl_distro.as_deref())
        .map_err(|error| error.splash_message().to_string())?;

    let bundled = bundled.ok_or_else(|| i18n::t(Msg::BootMissingBundle).to_string())?;
    let bundle_hash = read_bundle_hash(&bundled)?;
    let isolated_home = app_data_root()?.join("dsh-home");
    let windows_dsh_home = resolve_user_home(&isolated_home).path;
    let overlay_src = overlay::resolve_overlay_source(app.path().resource_dir().ok().as_deref());
    let overlay_for_provision = notify.map(|_| overlay_src.as_path());
    let notify_url = notify.map(|server| server.url.as_str());

    let wsl_paths = ensure_wsl_runtime(
        &runner,
        &distro.name,
        &bundled,
        &bundle_hash,
        &windows_dsh_home,
        overlay_for_provision,
        notify_url,
        {
            let progress = Arc::clone(&progress);
            move |event| progress(event)
        },
    )
    .await?;

    let host_overlay = notify.map(|server| HostOverlay {
        // Linux patch path is already on `wsl_paths.linux_patch`; Windows
        // `patch_file` is unused by `spawn_wsl_web_host`.
        patch_file: PathBuf::new(),
        notify_url: server.url.clone(),
    });

    progress(ProvisionEvent::Status(i18n::t(Msg::StatusStartWeb).into()));
    let host = spawn_wsl_web_host(&wsl_paths, host_overlay.as_ref(), &runner).await?;
    Ok(DesktopRuntime::start_wsl(host, wsl_paths))
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
