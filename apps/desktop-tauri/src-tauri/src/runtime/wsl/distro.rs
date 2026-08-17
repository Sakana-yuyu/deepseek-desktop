//! Parse `wsl.exe -l -v` output and select an eligible WSL2 distro.

const MSG_MISSING_WSL: &str = "未检测到 WSL。请安装 WSL2 后再将运行环境设为 WSL。";
const MSG_WSL1_ONLY: &str = "当前发行版是 WSL1。请执行 wsl --set-version <发行版> 2。";
const MSG_DOCKER_DEFAULT: &str =
    "默认 WSL 发行版是 Docker。请执行 wsl --set-default <Ubuntu 发行版名>。";
const MSG_NONE_ELIGIBLE: &str = "没有可用的 WSL2 发行版（已跳过 docker-desktop）。";

/// One row from `wsl.exe -l -v`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslDistro {
    pub name: String,
    pub version: u32,
    pub is_default: bool,
}

/// Why no eligible WSL2 distro could be selected for Host launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WslSelectError {
    MissingWsl(String),
    Wsl1Only(String),
    DockerDefault(String),
    NamedMissing { requested: String, message: String },
    NoneEligible(String),
}

impl WslSelectError {
    /// Chinese splash text for this selection failure.
    pub fn splash_message(&self) -> &str {
        match self {
            Self::MissingWsl(message)
            | Self::Wsl1Only(message)
            | Self::DockerDefault(message)
            | Self::NoneEligible(message) => message,
            Self::NamedMissing { message, .. } => message,
        }
    }

    /// Splash when `wsl.exe` is missing from PATH.
    pub fn missing_wsl() -> Self {
        Self::MissingWsl(MSG_MISSING_WSL.to_string())
    }

    fn wsl1_only() -> Self {
        Self::Wsl1Only(MSG_WSL1_ONLY.to_string())
    }

    fn docker_default() -> Self {
        Self::DockerDefault(MSG_DOCKER_DEFAULT.to_string())
    }

    fn named_missing(requested: &str) -> Self {
        Self::NamedMissing {
            requested: requested.to_string(),
            message: format!(
                "找不到 WSL 发行版 {requested}。请检查 desktop-settings.json 的 wslDistro。"
            ),
        }
    }

    fn none_eligible() -> Self {
        Self::NoneEligible(MSG_NONE_ELIGIBLE.to_string())
    }
}

/// Decode `wsl.exe -l -v` stdout as UTF-16LE (BOM optional) or UTF-8.
pub fn decode_wsl_list_stdout(bytes: &[u8]) -> String {
    if let Some(text) = try_decode_utf16le(bytes) {
        return text;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Parse decoded `wsl.exe -l -v` text into distro rows.
pub fn parse_wsl_list(text: &str) -> Vec<WslDistro> {
    let mut distros = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let header = trimmed.to_ascii_uppercase();
        if header.starts_with("NAME") && header.contains("VERSION") {
            continue;
        }
        let is_default = trimmed.starts_with('*');
        let body = if is_default {
            trimmed[1..].trim_start()
        } else {
            trimmed
        };
        let mut parts = body.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let Some(_state) = parts.next() else {
            continue;
        };
        let Some(version_text) = parts.next() else {
            continue;
        };
        let Ok(version) = version_text.parse::<u32>() else {
            continue;
        };
        distros.push(WslDistro {
            name: name.to_string(),
            version,
            is_default,
        });
    }
    distros
}

/// Distros that must not be chosen as the implicit default Host environment.
pub fn is_skipped_distro(name: &str) -> bool {
    matches!(name, "docker-desktop" | "docker-desktop-data")
}

/// Pick a WSL2 distro: explicit `requested` name, otherwise the starred default.
pub fn select_distro<'a>(
    distros: &'a [WslDistro],
    requested: Option<&str>,
) -> Result<&'a WslDistro, WslSelectError> {
    if let Some(name) = requested {
        return match distros.iter().find(|distro| distro.name == name) {
            Some(distro) if distro.version >= 2 => Ok(distro),
            Some(_) => Err(WslSelectError::wsl1_only()),
            None => Err(WslSelectError::named_missing(name)),
        };
    }

    let Some(default) = distros.iter().find(|distro| distro.is_default) else {
        return distros
            .iter()
            .find(|distro| distro.version >= 2 && !is_skipped_distro(&distro.name))
            .ok_or_else(|| {
                if distros.iter().any(|distro| distro.version < 2) {
                    WslSelectError::wsl1_only()
                } else {
                    WslSelectError::none_eligible()
                }
            });
    };

    if is_skipped_distro(&default.name) {
        return Err(WslSelectError::docker_default());
    }
    if default.version < 2 {
        return Err(WslSelectError::wsl1_only());
    }
    Ok(default)
}

fn try_decode_utf16le(bytes: &[u8]) -> Option<String> {
    let (data, forced) = if bytes.starts_with(&[0xFF, 0xFE]) {
        (&bytes[2..], true)
    } else {
        (bytes, false)
    };
    if data.is_empty() {
        return if forced { Some(String::new()) } else { None };
    }
    if data.len() % 2 != 0 {
        return if forced {
            Some(decode_utf16le_lossy(data))
        } else {
            None
        };
    }
    if !forced && !looks_like_utf16le(data) {
        return None;
    }
    Some(decode_utf16le_lossy(data))
}

fn looks_like_utf16le(data: &[u8]) -> bool {
    let pairs = data.len() / 2;
    if pairs == 0 {
        return false;
    }
    let zero_high = data.chunks_exact(2).filter(|pair| pair[1] == 0).count();
    zero_high * 2 >= pairs
}

fn decode_utf16le_lossy(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parses_utf16_starred_ubuntu() {
        let raw = utf16le(
            "  NAME              STATE           VERSION\r\n* Ubuntu            Running         2\r\n  docker-desktop    Running         2\r\n",
        );
        let text = decode_wsl_list_stdout(&raw);
        let list = parse_wsl_list(&text);
        let selected = select_distro(&list, None).unwrap();
        assert_eq!(selected.name, "Ubuntu");
        assert_eq!(selected.version, 2);
        assert!(selected.is_default);
    }

    #[test]
    fn rejects_docker_desktop_default() {
        let text = "  NAME                 STATE           VERSION\n* docker-desktop       Running         2\n  Ubuntu               Stopped         2\n";
        let err = select_distro(&parse_wsl_list(text), None).unwrap_err();
        assert!(err.splash_message().contains("wsl --set-default"));
    }

    #[test]
    fn requested_ubuntu_wins_over_docker_default() {
        let text = "* docker-desktop  Running  2\n  Ubuntu          Stopped  2\n";
        let list = parse_wsl_list(text);
        let selected = select_distro(&list, Some("Ubuntu")).unwrap();
        assert_eq!(selected.name, "Ubuntu");
    }

    #[test]
    fn rejects_wsl1_only() {
        let text = "* Ubuntu  Running  1\n";
        let err = select_distro(&parse_wsl_list(text), None).unwrap_err();
        assert!(err.splash_message().contains("WSL1"));
    }

    #[test]
    fn splash_messages_match_brief() {
        assert_eq!(
            WslSelectError::missing_wsl().splash_message(),
            "未检测到 WSL。请安装 WSL2 后再将运行环境设为 WSL。"
        );
        assert_eq!(
            WslSelectError::wsl1_only().splash_message(),
            "当前发行版是 WSL1。请执行 wsl --set-version <发行版> 2。"
        );
        assert_eq!(
            WslSelectError::docker_default().splash_message(),
            "默认 WSL 发行版是 Docker。请执行 wsl --set-default <Ubuntu 发行版名>。"
        );
        assert_eq!(
            WslSelectError::named_missing("Debian").splash_message(),
            "找不到 WSL 发行版 Debian。请检查 desktop-settings.json 的 wslDistro。"
        );
        assert_eq!(
            WslSelectError::none_eligible().splash_message(),
            "没有可用的 WSL2 发行版（已跳过 docker-desktop）。"
        );
    }
}
