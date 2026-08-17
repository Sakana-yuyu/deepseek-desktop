use std::path::{Path, PathBuf};
use std::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prevent spawned subprocesses from opening a visible console on Windows.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Put the Host in its own process group so stop can signal the whole tree.
pub fn isolate_host_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }
    let _ = cmd;
}

/// Terminate `pid` and every descendant. Used when Drop may not run (`app.exit`).
pub fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/T", "/PID", &pid.to_string()]);
        hide_console(&mut cmd);
        let _ = cmd.status();
    }
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

/// Record the Host pid and Node image so a later launch can reap an orphan.
///
/// WSL callers pass the Windows `wsl.exe` stub pid and the Linux Node image
/// path (`Path::new(linux_node)`). [`reclaim_stale_host`] will not match that
/// pair (Windows image ≠ Linux path), so live WSL reaping stays on
/// `HostHandle::stop`.
pub fn write_host_pid(path: &Path, pid: u32, node: &Path) -> Result<(), String> {
    std::fs::write(path, format!("{pid}\n{}\n", node.display())).map_err(|e| e.to_string())
}

/// Parse a `host.pid` file written by [`write_host_pid`].
pub fn parse_host_pid(raw: &str) -> Option<(u32, PathBuf)> {
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let node = lines.next()?.trim();
    if pid == 0 || node.is_empty() {
        return None;
    }
    Some((pid, PathBuf::from(node)))
}

/// Kill a previous Host tree when its recorded Node image still matches.
pub fn reclaim_stale_host(path: &Path) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let _ = std::fs::remove_file(path);
    let Some((pid, node)) = parse_host_pid(&raw) else {
        return;
    };
    if !host_pid_matches(pid, &node) {
        return;
    }
    kill_process_tree(pid);
}

fn host_pid_matches(pid: u32, expected_node: &Path) -> bool {
    let Some(image) = process_image_path(pid) else {
        return false;
    };
    crate::runtime::env_path::path_eq(&image, expected_node)
}

#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 512];
    let mut size = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    ok.ok()?;
    let text = String::from_utf16_lossy(&buf[..size as usize]);
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

#[cfg(unix)]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .or_else(|| {
            let raw = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).ok()?;
            let first = raw.split('\0').next()?.trim();
            if first.is_empty() {
                None
            } else {
                Some(PathBuf::from(first))
            }
        })
}

#[cfg(not(any(windows, unix)))]
fn process_image_path(_pid: u32) -> Option<PathBuf> {
    None
}

/// Windows job that kills every assigned process when the last handle closes.
#[cfg(windows)]
pub struct KillOnCloseJob {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for KillOnCloseJob {}

#[cfg(windows)]
impl KillOnCloseJob {
    pub fn create() -> Option<Self> {
        use std::mem::size_of;
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let handle = unsafe { CreateJobObjectW(None, None) }.ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let sized = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &raw const info as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if sized.is_err() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
            return None;
        }
        Some(Self { handle })
    }

    pub fn assign(&self, child: &Child) -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        let process = HANDLE(child.as_raw_handle());
        unsafe { AssignProcessToJobObject(self.handle, process) }.is_ok()
    }
}

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::parse_host_pid;
    use std::path::PathBuf;

    #[test]
    fn parses_pid_and_node_image() {
        assert_eq!(
            parse_host_pid("4321\nC:\\\\Program Files\\\\node.exe\n"),
            Some((4321, PathBuf::from(r"C:\\Program Files\\node.exe")))
        );
        assert_eq!(parse_host_pid("0\nC:\\\\node.exe\n"), None);
        assert_eq!(parse_host_pid("not-a-pid\nC:\\\\node.exe\n"), None);
    }
}
