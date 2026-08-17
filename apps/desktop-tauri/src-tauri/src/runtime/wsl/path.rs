//! Map Windows drive paths to WSL `/mnt/<drive>/...` mount paths.

use std::path::Path;

const MSG_WSL_UNC: &str = "WSL UNC 路径由发行版内路径表示，不要从 Windows UNC 启动 Host。";

/// Convert a Windows absolute drive path to a WSL `/mnt/<drive>/...` path.
pub fn windows_to_wsl_mount(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "路径包含无效的 Unicode 字符。".to_string())?;
    parse_windows_path(text)
}

fn parse_windows_path(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("路径为空。".into());
    }

    if is_unc_path(text) {
        return parse_unc_path(text);
    }

    if let Some((drive, rest)) = parse_drive_prefix(text) {
        return build_mnt_path(drive, rest);
    }

    Err("路径必须是绝对路径。".into())
}

fn is_unc_path(text: &str) -> bool {
    text.starts_with(r"\\") || text.starts_with("//")
}

fn parse_unc_path(text: &str) -> Result<String, String> {
    let after_prefix = text
        .strip_prefix(r"\\")
        .or_else(|| text.strip_prefix("//"))
        .ok_or_else(|| "路径必须是绝对路径。".to_string())?;

    let (server, _) = split_first_segment(after_prefix)?;
    if is_wsl_unc_server(server) {
        return Err(MSG_WSL_UNC.into());
    }

    Err(format!("不支持的 UNC 路径: \\\\{server}\\"))
}

fn parse_drive_prefix(text: &str) -> Option<(char, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    let drive = bytes[0] as char;
    if !drive.is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    if bytes[2] != b'\\' && bytes[2] != b'/' {
        return None;
    }
    Some((drive.to_ascii_lowercase(), &text[3..]))
}

fn build_mnt_path(drive: char, rest: &str) -> Result<String, String> {
    let mut out = format!("/mnt/{drive}");
    for segment in split_windows_segments(rest) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err("路径不能包含 ..".into());
        }
        out.push('/');
        out.push_str(segment);
    }
    Ok(out)
}

fn split_windows_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split(|c| c == '\\' || c == '/')
}

fn split_first_segment(path: &str) -> Result<(&str, &str), String> {
    let trimmed = path.trim_start_matches(|c| c == '\\' || c == '/');
    match trimmed.find(|c| c == '\\' || c == '/') {
        Some(idx) => Ok((&trimmed[..idx], &trimmed[idx..])),
        None => Ok((trimmed, "")),
    }
}

fn is_wsl_unc_server(server: &str) -> bool {
    server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
}

#[cfg(test)]
mod tests {
    use super::windows_to_wsl_mount;
    use std::path::Path;

    #[test]
    fn maps_drive_path_to_mnt() {
        assert_eq!(
            windows_to_wsl_mount(Path::new(r"D:\Project\foo")).unwrap(),
            "/mnt/d/Project/foo"
        );
        assert_eq!(
            windows_to_wsl_mount(Path::new(r"D:/Project/foo")).unwrap(),
            "/mnt/d/Project/foo"
        );
        assert_eq!(
            windows_to_wsl_mount(Path::new(r"C:\Users\me\AppData\Roaming\DeepSeek Harness"))
                .unwrap(),
            "/mnt/c/Users/me/AppData/Roaming/DeepSeek Harness"
        );
    }

    #[test]
    fn rejects_wsl_unc() {
        let err = windows_to_wsl_mount(Path::new(r"\\wsl$\Ubuntu\home\me")).unwrap_err();
        assert!(err.contains("UNC"));
    }

    #[test]
    fn rejects_wsl_localhost_unc() {
        let err =
            windows_to_wsl_mount(Path::new(r"\\wsl.localhost\Ubuntu\home\me")).unwrap_err();
        assert!(err.contains("UNC"));
    }

    #[test]
    fn rejects_relative_path() {
        let err = windows_to_wsl_mount(Path::new("Project/foo")).unwrap_err();
        assert_eq!(err, "路径必须是绝对路径。");

        let err = windows_to_wsl_mount(Path::new(r"D:Project\foo")).unwrap_err();
        assert_eq!(err, "路径必须是绝对路径。");
    }
}
