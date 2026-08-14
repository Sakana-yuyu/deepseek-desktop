//! When the desktop binary is copied or hard-linked as `dsh.exe`, run the CLI.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// Sidecar next to `dsh.exe` that names the selected Node, CLI entry, and home.
pub const LAUNCH_FILE: &str = "dsh-launch.json";

/// Launch record written by the PATH bridge and read by the `dsh.exe` trampoline.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshLaunchSpec {
    pub node: String,
    pub cli: String,
    pub dsh_home: String,
}

/// True when this process was started as the `dsh` CLI trampoline, not the GUI.
pub fn should_run_as_cli() -> bool {
    std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("dsh"))
}

/// Exec `node apps/cli/lib/bin.js` with the remaining argv. Returns the process exit code.
pub fn run() -> i32 {
    attach_parent_console();
    let exe_dir = match std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        Some(dir) => dir,
        None => {
            eprintln!("dsh: cannot resolve executable directory");
            return 1;
        }
    };
    let spec_path = exe_dir.join(LAUNCH_FILE);
    let spec = match read_launch_spec(&spec_path) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("dsh: {error}");
            return 1;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut command = Command::new(&spec.node);
    command
        .arg(&spec.cli)
        .args(&args)
        .env("DSH_HOME", &spec.dsh_home)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("dsh: failed to start {}: {error}", spec.node);
            1
        }
    }
}

/// Parse a launch sidecar; used by the trampoline and by PATH-bridge tests.
pub fn read_launch_spec(path: &Path) -> Result<DshLaunchSpec, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "missing launch sidecar {} ({e})",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}

fn attach_parent_console() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(test)]
mod tests {
    use super::{read_launch_spec, should_run_as_cli, DshLaunchSpec};
    use std::fs;

    #[test]
    fn desktop_test_runner_is_not_the_cli_trampoline() {
        assert!(!should_run_as_cli());
    }

    #[test]
    fn reads_camel_case_launch_sidecar() {
        let path = std::env::temp_dir().join(format!(
            "dsh-launch-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::write(
            &path,
            r#"{"node":"C:\\node.exe","cli":"C:\\bin.js","dshHome":"C:\\.dsh"}"#,
        )
        .unwrap();
        assert_eq!(
            read_launch_spec(&path).unwrap(),
            DshLaunchSpec {
                node: r"C:\node.exe".into(),
                cli: r"C:\bin.js".into(),
                dsh_home: r"C:\.dsh".into(),
            }
        );
        let _ = fs::remove_file(&path);
    }
}
