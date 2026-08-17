//! System tray: show/hide, close-behavior, check for updates, quit.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::chrome;
use crate::desktop_settings::{AgentEnvironment, CloseAction};
use crate::notify;
use crate::runtime::boot_log;
use crate::updater;

/// Install the tray icon and its menu. Closing the window uses the saved close action.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let close_min = MenuItem::with_id(
        app,
        "close-minimize",
        "关闭时最小化到托盘",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let close_exit =
        MenuItem::with_id(app, "close-exit", "关闭时退出程序", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let close_ask =
        MenuItem::with_id(app, "close-ask", "下次关闭时再询问", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let env_windows = MenuItem::with_id(
        app,
        "env-windows",
        "运行环境：Windows",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let env_wsl = MenuItem::with_id(
        app,
        "env-wsl",
        "运行环境：WSL（需重启）",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let update = MenuItem::with_id(app, "update", "检查更新", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
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
            &update,
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
                if chrome::apply_agent_environment(AgentEnvironment::Windows).is_ok() {
                    notify::toast(
                        app,
                        "DeepSeek Harness",
                        chrome::environment_changed_message(),
                    );
                }
            }
            "env-wsl" => {
                if chrome::apply_agent_environment(AgentEnvironment::Wsl).is_ok() {
                    notify::toast(
                        app,
                        "DeepSeek Harness",
                        chrome::environment_changed_message(),
                    );
                }
            }
            "update" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match updater::check_now(&app).await {
                        Ok(message) => notify::toast(&app, "DeepSeek Harness", &message),
                        Err(error) => {
                            boot_log::error(&format!("tray update failed: {error}"));
                            notify::toast(&app, "检查更新失败", &error);
                        }
                    }
                });
            }
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
