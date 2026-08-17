//! Map Windows drive paths to WSL `/mnt/<drive>/...` mount paths.

use std::path::{Component, Path, Prefix};

const MSG_WSL_UNC: &str = "WSL UNC 路径由发行版内路径表示，不要从 Windows UNC 启动 Host。";

/// Convert a Windows absolute drive path to a WSL `/mnt/<drive>/...` path.
pub fn windows_to_wsl_mount(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() {
        return Err("路径为空。".into());
    }
    if path.is_relative() {
        return Err("路径必须是绝对路径。".into());
    }

    let mut components = path.components();
    let prefix = components
        .next()
        .ok_or_else(|| "路径为空。".to_string())?;

    let drive = match prefix {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => char::from(d).to_ascii_lowercase(),
            Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => {
                if is_wsl_unc_server(server) {
                    return Err(MSG_WSL_UNC.into());
                }
                return Err(format!(
                    "不支持的 UNC 路径: \\\\{}\\",
                    server.to_string_lossy()
                ));
            }
            _ => return Err("仅支持 Windows 驱动器路径。".into()),
        },
        _ => return Err("仅支持 Windows 驱动器路径。".into()),
    };

    let mut out = format!("/mnt/{drive}");
    for component in components {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
            Component::Normal(part) => {
                out.push('/');
                out.push_str(&part.to_string_lossy());
            }
            Component::ParentDir => return Err("路径不能包含 ..".into()),
        }
    }
    Ok(out)
}

fn is_wsl_unc_server(server: &std::ffi::OsStr) -> bool {
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
}
