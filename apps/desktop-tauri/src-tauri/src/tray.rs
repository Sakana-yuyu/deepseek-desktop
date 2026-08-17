//! System tray: show/hide, close-behavior, check for updates, quit.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::chrome;
use crate::desktop_settings::{AgentEnvironment, CloseAction};
use crate::i18n::{self, Msg};
use crate::notify;
use crate::runtime::boot_log;
use crate::runtime::plugin_catalog;
use crate::updater;

/// Install the tray icon and its menu. Closing the window uses the saved close action.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let show = MenuItem::with_id(app, "show", i18n::t(Msg::TrayShow), true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let close_min = MenuItem::with_id(
        app,
        "close-minimize",
        i18n::t(Msg::TrayCloseMin),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let close_exit = MenuItem::with_id(
        app,
        "close-exit",
        i18n::t(Msg::TrayCloseExit),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let close_ask = MenuItem::with_id(
        app,
        "close-ask",
        i18n::t(Msg::TrayCloseAsk),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let env_windows = MenuItem::with_id(
        app,
        "env-windows",
        i18n::t(Msg::TrayEnvWindows),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let env_wsl = MenuItem::with_id(app, "env-wsl", i18n::t(Msg::TrayEnvWsl), true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let update = MenuItem::with_id(app, "update", i18n::t(Msg::TrayUpdate), true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let restart = MenuItem::with_id(
        app,
        "restart",
        i18n::t(Msg::TrayRestart),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let install_catalog = MenuItem::with_id(
        app,
        "install-catalog",
        i18n::t(Msg::TrayInstallCatalog),
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", i18n::t(Msg::TrayQuit), true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &close_min,
            &close_exit,
            &close_ask,
            &env_windows,
            &env_wsl,
            &install_catalog,
            &update,
            &restart,
            &quit,
        ],
    )
    .map_err(|e| e.to_string())?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default window icon is missing".to_string())?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("DeepSeek Harness")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => chrome::show_main(app),
            "close-minimize" => {
                let _ = chrome::remember_close_action(app, Some(CloseAction::Minimize));
            }
            "close-exit" => {
                let _ = chrome::remember_close_action(app, Some(CloseAction::Exit));
            }
            "close-ask" => {
                let _ = chrome::remember_close_action(app, None);
            }
            "env-windows" => {
                chrome::remember_agent_environment(app, AgentEnvironment::Windows);
            }
            "env-wsl" => {
                chrome::remember_agent_environment(app, AgentEnvironment::Wsl);
            }
            "install-catalog" => plugin_catalog::begin_from_tray(app),
            "update" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match updater::check_now(&app).await {
                        Ok(message) => notify::toast(&app, "DeepSeek Harness", &message),
                        Err(error) => {
                            boot_log::error(&format!("tray update failed: {error}"));
                            notify::toast(&app, i18n::t(Msg::TrayUpdateFailed), &error);
                        }
                    }
                });
            }
            "restart" => chrome::request_restart(app),
            "quit" => chrome::request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                chrome::show_main(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}
