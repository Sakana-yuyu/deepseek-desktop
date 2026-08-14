//! Frameless main window and custom title-bar commands.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::window_layout::resolve_controls_layout;

/// Create the frameless shell window that embeds `dsh web`.
pub fn open_main_window(app: &AppHandle, url: &str) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("main") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default window icon is missing".to_string())?;
    let init = format!(
        "window.__DSH_WEB_URL__ = {}; window.__DSH_CHROME__ = {};",
        serde_json::to_string(url).unwrap_or_else(|_| "\"\"".into()),
        serde_json::to_string(&resolve_controls_layout()).unwrap_or_else(|_| "{}".into())
    );

    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("shell.html".into()))
        .title("DeepSeek Harness")
        .inner_size(1280.0, 860.0)
        .center()
        .decorations(false)
        .visible(false)
        .initialization_script(&init);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        builder = builder.shadow(true);
    }

    let window = builder
        .icon(icon)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    let hide = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hide.hide();
        }
    });

    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// Focus or unhide the main window (single-instance and tray).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app
        .get_webview_window("main")
        .or_else(|| app.get_webview_window("splash"))
    {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
