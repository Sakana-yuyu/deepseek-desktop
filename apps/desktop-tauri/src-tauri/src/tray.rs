//! System tray: show/hide, check for updates, quit.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::chrome;
use crate::notify;
use crate::runtime::boot_log;
use crate::updater;

/// Install the tray icon and its menu. Closing the window hides to this icon.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let update = MenuItem::with_id(app, "update", "检查更新", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show, &update, &quit]).map_err(|e| e.to_string())?;

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
            "quit" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.destroy();
                }
                app.exit(0);
            }
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
