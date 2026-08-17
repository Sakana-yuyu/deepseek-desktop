//! WSL process runner seam and distro list parse/select.

pub mod distro;
mod launch;
mod path;
pub mod provision;
pub mod sys;

// Consumed by `supervisor::spawn_wsl_web_host`.
pub(crate) use launch::reject_windows_node;
pub use launch::{build_wsl_web_command, WslCommand, WslLaunchSpec};

pub use path::windows_to_wsl_mount;

pub use distro::{decode_wsl_list_stdout, parse_wsl_list, select_distro, WslSelectError};

pub use provision::{ensure_wsl_runtime, WslRuntimePaths};
pub use sys::SystemWslRunner;

/// Captured stdout/stderr/exit code from one `wsl.exe` invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: i32,
}

/// Injectable runner for `wsl.exe` (tests supply fixtures; production shells out).
pub trait WslRunner: Send + Sync {
    /// Run `wsl.exe` with the given arguments and return captured output.
    fn run(&self, args: &[&str]) -> Result<WslOutput, String>;
}
