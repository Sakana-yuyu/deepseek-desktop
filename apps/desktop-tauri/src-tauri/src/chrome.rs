//! Frameless main window, close preference, and custom title-bar commands.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::window::Color;
use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const DSH_BG: Color = Color(21, 21, 23, 255);

use crate::desktop_settings::{self, AgentEnvironment, CloseAction};
use crate::notify;
use crate::runtime::boot_log;
use crate::runtime::DesktopRuntime;
use crate::window_layout::resolve_controls_layout;

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// True when the user chose a full process exit (tray Quit or close-action Exit).
pub fn quit_requested() -> bool {
    QUIT_REQUESTED.load(Ordering::SeqCst)
}

/// Exit the process after marking quit so `ExitRequested` is not cancelled.
pub fn request_quit(app: &AppHandle) {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    stop_host(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
    app.exit(0);
}

/// Reap the Host Node tree. `app.exit` / `app.restart` skip `Drop`.
pub fn stop_host(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<DesktopRuntime>() {
        runtime.host.stop();
    }
}

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
        .background_color(DSH_BG)
        .theme(Some(Theme::Dark))
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

    let app_handle = window.app_handle().clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            on_close_requested(&app_handle);
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

/// Apply the saved close action, or ask once when none is stored.
pub fn on_close_requested(app: &AppHandle) {
    match desktop_settings::load().close_action {
        Some(CloseAction::Minimize) => hide_main(app),
        Some(CloseAction::Exit) => request_quit(app),
        None => {
            if let Err(error) = open_close_prompt(app) {
                boot_log::info(&format!("close prompt fallback hide: {error}"));
                hide_main(app);
            }
        }
    }
}

/// Persist a close action from the in-window prompt, then apply it.
#[tauri::command]
pub fn set_close_action(app: AppHandle, action: String) -> Result<(), String> {
    let parsed = parse_close_action(&action)?;
    remember_close_action(&app, parsed)?;
    hide_close_prompt(&app);
    match parsed {
        None => {}
        Some(CloseAction::Minimize) => hide_main(&app),
        Some(CloseAction::Exit) => request_quit(&app),
    }
    Ok(())
}

/// Persist a close action from the tray without immediately hiding or quitting.
pub fn remember_close_action(app: &AppHandle, action: Option<CloseAction>) -> Result<(), String> {
    save_close_action(action)?;
    let message = match action {
        Some(CloseAction::Minimize) => "关闭窗口将最小化到托盘",
        Some(CloseAction::Exit) => "关闭窗口将退出程序",
        None => "下次关闭窗口时会再询问",
    };
    notify::toast(app, "DeepSeek Harness", message);
    Ok(())
}

fn parse_close_action(action: &str) -> Result<Option<CloseAction>, String> {
    match action {
        "minimize" => Ok(Some(CloseAction::Minimize)),
        "exit" => Ok(Some(CloseAction::Exit)),
        "ask" => Ok(None),
        other => Err(format!("unknown close action: {other}")),
    }
}

fn save_close_action(action: Option<CloseAction>) -> Result<(), String> {
    let mut settings = desktop_settings::load();
    settings.close_action = action;
    desktop_settings::save(&settings)
}

fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn open_close_prompt(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is missing".to_string())?;
    main.eval("window.__DSH_CLOSE_PROMPT__?.show()")
        .map_err(|e| e.to_string())?;
    let _ = main.set_focus();
    Ok(())
}

fn hide_close_prompt(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval("window.__DSH_CLOSE_PROMPT__?.hide()");
    }
}

/// Toast copy when the tray changes the agent runtime target.
pub fn environment_changed_message() -> &'static str {
    "运行环境将在重启后生效"
}

/// Persist the agent runtime target from the tray without restarting the Host.
pub fn apply_agent_environment(value: AgentEnvironment) -> Result<(), String> {
    let mut settings = desktop_settings::load();
    settings.agent_environment = value;
    desktop_settings::save(&settings)
}

#[cfg(test)]
mod tests {
    use super::environment_changed_message;

    #[test]
    fn environment_changed_message_is_restart_toast() {
        assert_eq!(environment_changed_message(), "运行环境将在重启后生效");
    }
}
