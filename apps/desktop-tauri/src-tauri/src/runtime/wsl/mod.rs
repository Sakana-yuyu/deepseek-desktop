//! WSL process runner seam and distro list parse/select.

pub mod distro;
mod path;

// Re-exported for later WSL host tasks (provision/spawn).
#[allow(unused_imports)] // consumed by later spawn/provision tasks
pub use path::windows_to_wsl_mount;

// Re-exported for later WSL host tasks (`crate::runtime::wsl::select_distro`).
#[allow(unused_imports)] // consumed by later spawn/provision tasks
pub use distro::{
    decode_wsl_list_stdout, is_skipped_distro, parse_wsl_list, select_distro, WslDistro,
    WslSelectError,
};

/// Captured stdout/stderr/exit code from one `wsl.exe` invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)] // consumed by later spawn/provision tasks
pub struct WslOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: i32,
}

/// Injectable runner for `wsl.exe` (tests supply fixtures; production shells out).
#[allow(dead_code)] // consumed by later spawn/provision tasks
pub trait WslRunner {
    /// Run `wsl.exe` with the given arguments and return captured output.
    fn run(&self, args: &[&str]) -> Result<WslOutput, String>;
}
