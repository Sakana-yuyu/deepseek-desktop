//! Production `wsl.exe` runner used by the desktop boot path.

use std::io::ErrorKind;
use std::process::{Command, Stdio};

use super::{decode_wsl_list_stdout, WslOutput, WslRunner};
use crate::runtime::process::hide_console;

/// Spawns `wsl.exe` directly (never through `cmd.exe`).
pub struct SystemWslRunner;

impl WslRunner for SystemWslRunner {
    fn run(&self, args: &[&str]) -> Result<WslOutput, String> {
        let mut cmd = Command::new("wsl.exe");
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut cmd);
        let output = cmd.output().map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                "wsl.exe not found".to_string()
            } else {
                format!("failed to run wsl.exe: {error}")
            }
        })?;
        let stdout = if is_wsl_list_args(args) {
            decode_wsl_list_stdout(&output.stdout).into_bytes()
        } else {
            output.stdout
        };
        Ok(WslOutput {
            stdout,
            stderr: output.stderr,
            code: output.status.code().unwrap_or(-1),
        })
    }
}

fn is_wsl_list_args(args: &[&str]) -> bool {
    matches!(args, ["-l"] | ["-l", "-v"])
}

#[cfg(test)]
mod tests {
    use super::is_wsl_list_args;

    #[test]
    fn recognizes_list_argv() {
        assert!(is_wsl_list_args(&["-l"]));
        assert!(is_wsl_list_args(&["-l", "-v"]));
        assert!(!is_wsl_list_args(&["-d", "Ubuntu", "--exec", "true"]));
    }
}
