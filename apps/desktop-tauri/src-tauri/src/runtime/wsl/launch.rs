//! Build `wsl.exe` argv for `dsh web` without executing Windows `node.exe`.

const ERR_WINDOWS_NODE: &str = "禁止在 WSL 中执行 Windows node.exe";

/// Inputs for one WSL Host `dsh web` launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslLaunchSpec {
    pub distro: String,
    pub linux_node: String,
    pub linux_cli: String,
    pub linux_harness_root: String,
    pub linux_dsh_home: String,
    pub linux_path: String,
    pub linux_patch: Option<String>,
    pub notify_url: Option<String>,
    pub port: u16,
    pub host: String,
}

/// Resolved `wsl.exe` program and argument vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Build `wsl.exe` argv for `dsh web` inside the selected distro.
pub fn build_wsl_web_command(spec: &WslLaunchSpec) -> Result<WslCommand, String> {
    if spec.linux_node.contains('\\')
        || spec
            .linux_node
            .to_ascii_lowercase()
            .ends_with("node.exe")
    {
        return Err(ERR_WINDOWS_NODE.into());
    }

    let mut args = vec![
        "-d".into(),
        spec.distro.clone(),
        "--cd".into(),
        spec.linux_harness_root.clone(),
        "--exec".into(),
        "/usr/bin/env".into(),
        format!("PATH={}", spec.linux_path),
        format!("DSH_HOME={}", spec.linux_dsh_home),
        "NODE_ENV=production".into(),
    ];

    if let Some(url) = &spec.notify_url {
        args.push(format!("DSH_DESKTOP_NOTIFY_URL={url}"));
    }

    args.push(spec.linux_node.clone());
    args.push(spec.linux_cli.clone());
    args.push("web".into());

    if let Some(patch) = &spec.linux_patch {
        args.push("--patch".into());
        args.push(patch.clone());
    }

    args.push("--host".into());
    args.push("127.0.0.1".into());
    args.push("--port".into());
    args.push(spec.port.to_string());

    Ok(WslCommand {
        program: "wsl.exe".into(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_wsl_web_command, WslLaunchSpec};

    fn spec() -> WslLaunchSpec {
        WslLaunchSpec {
            distro: "Ubuntu".into(),
            linux_node: "/home/u/.local/share/dsh-desktop/runtime/node/bin/node".into(),
            linux_cli: "/home/u/.local/share/dsh-desktop/harness-versions/abc/apps/cli/lib/bin.js"
                .into(),
            linux_harness_root: "/home/u/.local/share/dsh-desktop/harness-versions/abc".into(),
            linux_dsh_home: "/home/u/.dsh".into(),
            linux_path: "/home/u/.local/share/dsh-desktop/runtime/node/bin:/usr/bin".into(),
            linux_patch: Some("/home/u/.dsh/desktop-overlay/cordis.yml".into()),
            notify_url: Some("http://127.0.0.1:17991/".into()),
            port: 17890,
            host: "127.0.0.1".into(),
        }
    }

    #[test]
    fn argv_uses_linux_node_and_patch() {
        let cmd = build_wsl_web_command(&spec()).unwrap();
        assert_eq!(cmd.program, "wsl.exe");
        assert!(cmd.args.windows(2).any(|w| w == ["-d", "Ubuntu"]));
        assert!(cmd.args.contains(&"--exec".into()));
        assert!(cmd.args.iter().any(|a| a.ends_with("/bin/node")));
        assert!(!cmd
            .args
            .iter()
            .any(|a| a.to_ascii_lowercase().ends_with("node.exe")));
        assert!(cmd.args.windows(2).any(|w| {
            w == [
                "--patch",
                "/home/u/.dsh/desktop-overlay/cordis.yml"
            ]
        }));
    }

    #[test]
    fn rejects_windows_node_exe() {
        let mut s = spec();
        s.linux_node = r"C:\Program Files\nodejs\node.exe".into();
        let err = build_wsl_web_command(&s).unwrap_err();
        assert!(err.contains("node.exe"));
    }
}
