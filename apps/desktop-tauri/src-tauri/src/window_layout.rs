//! OS-aware window-control placement for the custom title bar.
//!
//! Windows keeps minimize/maximize/close on the right. macOS keeps close/
//! minimize/maximize on the left. Linux reads the window-manager button
//! layout when available, including split left/right placements.

use serde::Serialize;

/// One title-bar control the shell HTML may render.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// Buttons assigned to each side of the title bar.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ControlsLayout {
    pub left: Vec<WindowButton>,
    pub right: Vec<WindowButton>,
    pub os: &'static str,
    pub titlebar_height: u32,
}

/// Resolve the live title-bar layout for this host.
///
/// `DSH_DESKTOP_BUTTON_LAYOUT` overrides the platform default using the
/// GNOME `left:right` token list (`close,minimize,maximize:` / `:minimize,maximize,close`).
pub fn resolve_controls_layout() -> ControlsLayout {
    let parsed = if let Ok(raw) = std::env::var("DSH_DESKTOP_BUTTON_LAYOUT") {
        parse_button_layout(&raw)
    } else {
        platform_button_layout()
    };
    ControlsLayout {
        left: parsed.0,
        right: parsed.1,
        os: current_os(),
        titlebar_height: titlebar_height(),
    }
}

fn current_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        other => other,
    }
}

fn titlebar_height() -> u32 {
    if cfg!(target_os = "macos") {
        32
    } else {
        36
    }
}

fn platform_button_layout() -> (Vec<WindowButton>, Vec<WindowButton>) {
    #[cfg(target_os = "macos")]
    {
        return parse_button_layout("close,minimize,maximize:");
    }
    #[cfg(target_os = "windows")]
    {
        return parse_button_layout(":minimize,maximize,close");
    }
    #[cfg(target_os = "linux")]
    {
        return linux_button_layout();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        parse_button_layout(":minimize,maximize,close")
    }
}

#[cfg(target_os = "linux")]
fn linux_button_layout() -> (Vec<WindowButton>, Vec<WindowButton>) {
    if let Some(raw) = gnome_button_layout() {
        return parse_button_layout(&raw);
    }
    if let Some(raw) = xfce_button_layout() {
        return parse_button_layout(&raw);
    }
    parse_button_layout(":minimize,maximize,close")
}

#[cfg(target_os = "linux")]
fn gnome_button_layout() -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    Some(raw.trim().trim_matches('\'').trim_matches('"').to_string())
}

#[cfg(target_os = "linux")]
fn xfce_button_layout() -> Option<String> {
    let output = std::process::Command::new("xfconf-query")
        .args(["-c", "xfwm4", "-p", "/general/button_layout"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    Some(normalize_xfce_layout(&raw))
}

/// Map XFCE `O|HMC` tokens onto the GNOME `left:right` list.
#[cfg(target_os = "linux")]
fn normalize_xfce_layout(raw: &str) -> String {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut side = &mut left;
    for ch in raw.chars() {
        match ch {
            '|' => side = &mut right,
            'H' => side.push("minimize"),
            'M' => side.push("maximize"),
            'C' => side.push("close"),
            _ => {}
        }
    }
    format!("{}:{}", left.join(","), right.join(","))
}

/// Parse a GNOME-style `left:right` button-layout string.
pub fn parse_button_layout(raw: &str) -> (Vec<WindowButton>, Vec<WindowButton>) {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"');
    let (left_raw, right_raw) = match trimmed.split_once(':') {
        Some(parts) => parts,
        None => ("", trimmed),
    };
    (parse_side(left_raw), parse_side(right_raw))
}

fn parse_side(raw: &str) -> Vec<WindowButton> {
    raw.split(',')
        .filter_map(|token| match token.trim().to_ascii_lowercase().as_str() {
            "minimize" | "min" => Some(WindowButton::Minimize),
            "maximize" | "max" => Some(WindowButton::Maximize),
            "close" => Some(WindowButton::Close),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_button_layout, WindowButton};

    #[test]
    fn windows_default_keeps_controls_on_the_right() {
        assert_eq!(
            parse_button_layout(":minimize,maximize,close"),
            (
                vec![],
                vec![
                    WindowButton::Minimize,
                    WindowButton::Maximize,
                    WindowButton::Close
                ]
            )
        );
    }

    #[test]
    fn macos_default_keeps_traffic_lights_on_the_left() {
        assert_eq!(
            parse_button_layout("close,minimize,maximize:"),
            (
                vec![
                    WindowButton::Close,
                    WindowButton::Minimize,
                    WindowButton::Maximize
                ],
                vec![]
            )
        );
    }

    #[test]
    fn linux_may_split_close_left_and_maximize_right() {
        assert_eq!(
            parse_button_layout("close:maximize"),
            (vec![WindowButton::Close], vec![WindowButton::Maximize])
        );
    }

    #[test]
    fn ignores_app_menu_and_unknown_tokens() {
        assert_eq!(
            parse_button_layout("appmenu:minimize,maximize,close"),
            (
                vec![],
                vec![
                    WindowButton::Minimize,
                    WindowButton::Maximize,
                    WindowButton::Close
                ]
            )
        );
    }
}
