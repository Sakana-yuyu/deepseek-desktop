use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::app_data_root;
use super::boot_log;
use super::config::DEFAULT_WEB_PORT;
use super::process::{
    hide_console, isolate_host_group, kill_process_tree, reclaim_stale_host, write_host_pid,
};
use super::provision::RuntimePaths;
use super::wsl::{build_wsl_web_command, WslLaunchSpec, WslRunner, WslRuntimePaths};

/// Maximum broken plugins one boot disables before giving up on the Host.
const MAX_PLUGIN_RESCUES: usize = 4;
/// Bound for reading the Linux pid handshake from WSL stderr.
const WSL_PID_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Linux Host identity inside a WSL distro (pid discovered via stderr handshake).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslSession {
    pub distro: String,
    pub linux_pid: u32,
}

/// Running `dsh web` child, bound port, and verified base URL.
pub struct HostHandle {
    pub port: u16,
    pub web_url: String,
    /// Plugin entry ids whose load failure was bypassed through a rescue
    /// `--patch` this session; empty when the Host started clean.
    pub disabled_plugins: Vec<String>,
    /// Set when the Host runs inside WSL; `None` for the Windows `node.exe` path.
    /// Cleared on the first successful `stop` so Drop does not wait again.
    pub wsl: Mutex<Option<WslSession>>,
    child: Arc<Mutex<Option<Child>>>,
    #[cfg(windows)]
    job: Mutex<Option<super::process::KillOnCloseJob>>,
}

impl HostHandle {
    /// Stop the Host Node tree. Safe to call more than once, including before
    /// `app.exit` / `app.restart`, which do not run `Drop`.
    pub fn stop(&self) {
        let session = self
            .wsl
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(session) = session {
            stop_wsl_linux_host(&session);
        }
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        #[cfg(windows)]
        if let Ok(mut job) = self.job.lock() {
            job.take();
        }
        let _ = std::fs::remove_file(host_pid_path());
    }
}

impl Drop for HostHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Extra Host flags the desktop shell injects without editing Harness packages.
pub struct HostOverlay {
    pub patch_file: std::path::PathBuf,
    pub notify_url: String,
}

/// Spawn `dsh web --host 127.0.0.1 --port <port>` and wait until HTTP responds.
/// A Host that dies naming a loader entry (`failed to apply loader entry <id>`)
/// is respawned with that plugin disabled through a rescue `--patch` overlay,
/// so one broken community plugin cannot keep the desktop closed; the disable
/// lasts only this boot, so a fixed or updated plugin loads again on restart.
pub async fn spawn_web_host(
    paths: &RuntimePaths,
    overlay: Option<&HostOverlay>,
    host_path: &str,
) -> Result<HostHandle, String> {
    if !paths.cli_entry.is_file() {
        return Err(format!(
            "harness CLI 缺失: {} — 请确认安装包内已包含 apps/cli/lib",
            paths.cli_entry.display()
        ));
    }

    reclaim_stale_host(&host_pid_path());
    let port = pick_port(DEFAULT_WEB_PORT)?;
    let web_url = format!("http://127.0.0.1:{port}/");
    let mut disabled_plugins: Vec<String> = Vec::new();
    let mut last_error = String::new();

    for _ in 0..=MAX_PLUGIN_RESCUES {
        let rescue_patch = (!disabled_plugins.is_empty())
            .then(|| write_rescue_patch(&disabled_plugins))
            .transpose()?;
        boot_log::info(&format!(
            "spawning dsh web node={} cli={} port={port} rescue={}",
            paths.node_binary.display(),
            paths.cli_entry.display(),
            if disabled_plugins.is_empty() {
                "none".to_string()
            } else {
                disabled_plugins.join(",")
            }
        ));
        let child = spawn_child(paths, port, overlay, host_path, rescue_patch.as_deref())?;
        let pid = child.id();
        #[cfg(windows)]
        let job = attach_host_job(&child);
        if let Err(error) = write_host_pid(&host_pid_path(), pid, &paths.node_binary) {
            boot_log::info(&format!("host pid file skipped: {error}"));
        }
        let child_handle = Arc::new(Mutex::new(Some(child)));

        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        if let Some(stderr) = child_handle
            .lock()
            .map_err(|e| e.to_string())?
            .as_mut()
            .and_then(|c| c.stderr.take())
        {
            let lines = Arc::clone(&stderr_lines);
            std::thread::spawn(move || drain_lines(stderr, lines));
        }

        if let Some(stdout) = child_handle
            .lock()
            .map_err(|e| e.to_string())?
            .as_mut()
            .and_then(|c| c.stdout.take())
        {
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for _ in reader.lines().flatten() {}
            });
        }

        if let Err(error) = wait_for_http(
            &web_url,
            &child_handle,
            &stderr_lines,
            Duration::from_secs(120),
            None,
        )
        .await
        {
            if let Ok(mut guard) = child_handle.lock() {
                if let Some(mut child) = guard.take() {
                    kill_process_tree(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            let _ = std::fs::remove_file(host_pid_path());
            last_error = error.clone();
            match failing_loader_entry(&error)
                .filter(|entry| !disabled_plugins.contains(entry))
            {
                Some(entry) => {
                    boot_log::error(&format!(
                        "plugin {entry} failed to load; retrying with it disabled"
                    ));
                    disabled_plugins.push(entry);
                    continue;
                }
                None => return Err(error),
            }
        }
        boot_log::info(&format!("health check passed url={web_url}"));

        return Ok(HostHandle {
            port,
            web_url,
            disabled_plugins,
            wsl: Mutex::new(None),
            child: child_handle,
            #[cfg(windows)]
            job: Mutex::new(job),
        });
    }
    Err(last_error)
}

/// Spawn `dsh web` as a Linux Node process inside WSL and wait until HTTP responds.
///
/// `runner` is reserved for callers that already hold a `WslRunner`; the long-lived
/// Host is spawned via `wsl.exe` directly so stdout/stderr stay piped.
pub async fn spawn_wsl_web_host(
    paths: &WslRuntimePaths,
    overlay: Option<&HostOverlay>,
    _runner: &dyn WslRunner,
) -> Result<HostHandle, String> {
    reclaim_stale_host(&host_pid_path());
    let port = pick_port(DEFAULT_WEB_PORT)?;
    let web_url = format!("http://127.0.0.1:{port}/");

    let spec = WslLaunchSpec {
        distro: paths.distro.clone(),
        linux_node: paths.linux_node.clone(),
        linux_cli: paths.linux_cli.clone(),
        linux_harness_root: paths.linux_harness_root.clone(),
        linux_dsh_home: paths.linux_dsh_home.clone(),
        linux_path: paths.linux_path.clone(),
        linux_patch: paths.linux_patch.clone(),
        notify_url: overlay.map(|o| o.notify_url.clone()),
        port,
        host: "127.0.0.1".into(),
    };

    boot_log::info(&format!(
        "spawning wsl dsh web distro={} node={} cli={} port={port}",
        paths.distro, paths.linux_node, paths.linux_cli
    ));

    let command = build_wsl_web_command(&spec)?;
    let mut child = spawn_wsl_child(&command)?;
    let stub_pid = child.id();
    #[cfg(windows)]
    let job = attach_host_job(&child);

    let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let linux_pid = match take_linux_pid_and_drain_stderr(&mut child, &stderr_lines).await {
        Ok(pid) => pid,
        Err(error) => {
            reap_wsl_stub_only(&mut child);
            return Err(error);
        }
    };

    if let Err(error) = write_host_pid(
        &host_pid_path(),
        stub_pid,
        Path::new(&paths.linux_node),
    ) {
        boot_log::info(&format!("host pid file skipped: {error}"));
    }

    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for _ in reader.lines().flatten() {}
        });
    }

    let child_handle = Arc::new(Mutex::new(Some(child)));
    let session = WslSession {
        distro: paths.distro.clone(),
        linux_pid,
    };
    let wsl_timeout = "请检查 WSL 的 localhost 转发（localhostForwarding）。";
    if let Err(error) = wait_for_http(
        &web_url,
        &child_handle,
        &stderr_lines,
        Duration::from_secs(120),
        Some(wsl_timeout),
    )
    .await
    {
        reap_wsl_session_and_stub(&session, &child_handle);
        return Err(error);
    }

    boot_log::info(&format!(
        "health check passed url={web_url} linux_pid={linux_pid}"
    ));

    Ok(HostHandle {
        port,
        web_url,
        disabled_plugins: Vec::new(),
        wsl: Mutex::new(Some(session)),
        child: child_handle,
        #[cfg(windows)]
        job: Mutex::new(job),
    })
}

/// Parse the Linux Host pid from stderr handshake text.
///
/// Scans every line and returns the first that is entirely a decimal pid so a
/// leading `wsl.exe` diagnostic does not hide `echo $$`.
pub fn parse_linux_pid_from_stderr(stderr: &str) -> Option<u32> {
    stderr.lines().find_map(parse_linux_pid_line)
}

fn parse_linux_pid_line(line: &str) -> Option<u32> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

/// `wsl.exe` argv that sends `SIGTERM` to the Linux Host pid (never `--terminate`).
pub fn wsl_stop_args(distro: &str, linux_pid: u32) -> Vec<String> {
    vec![
        "-d".into(),
        distro.into(),
        "--exec".into(),
        "kill".into(),
        "-TERM".into(),
        linux_pid.to_string(),
    ]
}

fn wsl_kill_args(distro: &str, linux_pid: u32) -> Vec<String> {
    vec![
        "-d".into(),
        distro.into(),
        "--exec".into(),
        "kill".into(),
        "-KILL".into(),
        linux_pid.to_string(),
    ]
}

fn wsl_pid_alive_args(distro: &str, linux_pid: u32) -> Vec<String> {
    vec![
        "-d".into(),
        distro.into(),
        "--exec".into(),
        "kill".into(),
        "-0".into(),
        linux_pid.to_string(),
    ]
}

fn stop_wsl_linux_host(session: &WslSession) {
    let _ = run_wsl_argv(&wsl_stop_args(&session.distro, session.linux_pid));
    std::thread::sleep(Duration::from_secs(3));
    if wsl_linux_pid_alive(&session.distro, session.linux_pid) {
        let _ = run_wsl_argv(&wsl_kill_args(&session.distro, session.linux_pid));
    }
}

fn wsl_linux_pid_alive(distro: &str, linux_pid: u32) -> bool {
    run_wsl_argv(&wsl_pid_alive_args(distro, linux_pid))
        .map(|code| code == 0)
        .unwrap_or(false)
}

fn run_wsl_argv(args: &[String]) -> Result<i32, String> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(args);
    hide_console(&mut cmd);
    let status = cmd
        .status()
        .map_err(|e| format!("无法执行 wsl.exe: {e}"))?;
    Ok(status.code().unwrap_or(-1))
}

fn reap_wsl_stub_only(child: &mut Child) {
    kill_process_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(host_pid_path());
}

fn reap_wsl_session_and_stub(session: &WslSession, child: &Arc<Mutex<Option<Child>>>) {
    stop_wsl_linux_host(session);
    if let Ok(mut guard) = child.lock() {
        if let Some(mut child) = guard.take() {
            kill_process_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    let _ = std::fs::remove_file(host_pid_path());
}

fn spawn_wsl_child(command: &super::wsl::WslCommand) -> Result<Child, String> {
    let mut cmd = Command::new(&command.program);
    cmd.args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("无法启动 WSL dsh web: {e}"))
}

/// Read stderr until the first parseable pid line, within [`WSL_PID_HANDSHAKE_TIMEOUT`].
/// Remaining lines keep draining into `stderr_lines` for later failure messages.
async fn take_linux_pid_and_drain_stderr(
    child: &mut Child,
    stderr_lines: &Arc<Mutex<Vec<String>>>,
) -> Result<u32, String> {
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "WSL Host stderr 不可用".to_string())?;
    let lines = Arc::clone(stderr_lines);
    let join = tokio::task::spawn_blocking(move || {
        read_linux_pid_handshake(stderr, lines, WSL_PID_HANDSHAKE_TIMEOUT)
    });
    join.await
        .map_err(|e| format!("WSL Host pid handshake 任务失败: {e}"))?
}

fn read_linux_pid_handshake<R: std::io::Read + Send + 'static>(
    stderr: R,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    timeout: Duration,
) -> Result<u32, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<u32, String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let mut sent = false;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end().to_string();
                    if !trimmed.is_empty() {
                        if let Ok(mut guard) = stderr_lines.lock() {
                            guard.push(trimmed.clone());
                            if guard.len() > 64 {
                                let drop = guard.len() - 64;
                                guard.drain(0..drop);
                            }
                        }
                    }
                    if !sent {
                        if let Some(pid) = parse_linux_pid_line(&trimmed) {
                            let _ = tx.send(Ok(pid));
                            sent = true;
                        }
                    }
                }
                Err(error) => {
                    if !sent {
                        let _ = tx.send(Err(format!("无法读取 WSL Host pid: {error}")));
                    }
                    return;
                }
            }
        }
        if !sent {
            let preview = stderr_lines
                .lock()
                .map(|lines| lines.join("\n"))
                .unwrap_or_default();
            let detail = if preview.is_empty() {
                "无 stderr 输出".to_string()
            } else {
                preview
            };
            let _ = tx.send(Err(format!(
                "无法解析 WSL Host pid（handshake）: {detail}"
            )));
        }
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err("等待 WSL Host pid handshake 超时".into())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("WSL Host pid handshake 通道已断开".into())
        }
    }
}

/// Write the rescue `--patch` overlay that disables the given plugin entry ids.
fn write_rescue_patch(ids: &[String]) -> Result<PathBuf, String> {
    let path = app_data_root()?.join("plugin-rescue.patch.yml");
    std::fs::write(&path, rescue_patch_body(ids))
        .map_err(|e| format!("无法写入 {}: {e}", path.display()))?;
    Ok(path)
}

/// One `disabled: true` patch row per plugin entry id.
fn rescue_patch_body(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("- id: {id}\n  disabled: true\n"))
        .collect()
}

/// The plugin entry id named by a loader failure message, e.g.
/// "failed to apply loader entry dsh-plugins-catalog (…): invalid plugin".
/// The innermost (last) occurrence is taken; nested causes repeat the id.
fn failing_loader_entry(message: &str) -> Option<String> {
    const NEEDLE: &str = "failed to apply loader entry ";
    let start = message.rfind(NEEDLE)? + NEEDLE.len();
    let id: String = message[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ':')
        .collect();
    (!id.is_empty()).then_some(id)
}

fn host_pid_path() -> std::path::PathBuf {
    app_data_root()
        .map(|root| root.join("host.pid"))
        .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop-host.pid"))
}

#[cfg(windows)]
fn attach_host_job(child: &Child) -> Option<super::process::KillOnCloseJob> {
    let job = super::process::KillOnCloseJob::create()?;
    if job.assign(child) {
        Some(job)
    } else {
        None
    }
}

fn spawn_child(
    paths: &RuntimePaths,
    port: u16,
    overlay: Option<&HostOverlay>,
    host_path: &str,
    rescue_patch: Option<&Path>,
) -> Result<Child, String> {
    let mut cmd = Command::new(&paths.node_binary);
    cmd.arg(&paths.cli_entry).arg("web");
    if let Some(overlay) = overlay {
        cmd.arg("--patch").arg(&overlay.patch_file);
        cmd.env("DSH_DESKTOP_NOTIFY_URL", &overlay.notify_url);
    }
    if let Some(patch) = rescue_patch {
        cmd.arg("--patch").arg(patch);
    }
    cmd.arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .env("DSH_HOME", &paths.dsh_home)
        .env("PATH", host_path)
        .env("NODE_ENV", "production")
        .current_dir(&paths.harness_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    isolate_host_group(&mut cmd);
    hide_console(&mut cmd);

    cmd.spawn().map_err(|e| format!("无法启动 dsh web: {e}"))
}

fn drain_lines<R: std::io::Read>(reader: R, sink: Arc<Mutex<Vec<String>>>) {
    let reader = BufReader::new(reader);
    for line in reader.lines().flatten() {
        if let Ok(mut guard) = sink.lock() {
            guard.push(line);
            if guard.len() > 64 {
                let drop = guard.len() - 64;
                guard.drain(0..drop);
            }
        }
    }
}

fn child_exit_code(child: &Arc<Mutex<Option<Child>>>) -> Option<i32> {
    let mut guard = child.lock().ok()?;
    let child = guard.as_mut()?;
    match child.try_wait().ok()? {
        Some(status) => Some(status.code().unwrap_or(-1)),
        None => None,
    }
}

fn format_child_failure(stderr_lines: &Arc<Mutex<Vec<String>>>, exit_code: i32) -> String {
    let tail = stderr_lines
        .lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();
    if tail.is_empty() {
        format!("dsh web 进程已退出 (code {exit_code})")
    } else {
        format!("dsh web 进程已退出 (code {exit_code})\n{tail}")
    }
}

async fn wait_for_http(
    url: &str,
    child: &Arc<Mutex<Option<Child>>>,
    stderr_lines: &Arc<Mutex<Vec<String>>>,
    timeout: Duration,
    timeout_detail: Option<&str>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let deadline = tokio::time::Instant::now() + timeout;
    let mut logged_failure = false;

    loop {
        if tokio::time::Instant::now() >= deadline {
            if let Some(code) = child_exit_code(child) {
                return Err(format_child_failure(stderr_lines, code));
            }
            return Err(match timeout_detail {
                Some(detail) => format!("等待 {url} 就绪超时。{detail}"),
                None => format!("等待 {url} 就绪超时"),
            });
        }

        if let Some(code) = child_exit_code(child) {
            return Err(format_child_failure(stderr_lines, code));
        }

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                boot_log::info(&format!(
                    "http ready status={} url={url}",
                    response.status()
                ));
                return Ok(());
            }
            Ok(response) => {
                boot_log::info(&format!(
                    "health probe non-success status={} url={url}",
                    response.status()
                ));
            }
            Err(err) => {
                if !logged_failure {
                    boot_log::info(&format!("health probe failed url={url} err={err}"));
                    logged_failure = true;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

fn pick_port(preferred: u16) -> Result<u16, String> {
    for port in preferred..preferred.saturating_add(10) {
        if port_free(port) {
            return Ok(port);
        }
    }
    Err(format!("端口 {preferred}–{} 均被占用", preferred + 9))
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{
        failing_loader_entry, parse_linux_pid_from_stderr, read_linux_pid_handshake,
        rescue_patch_body, wsl_stop_args,
    };
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn extracts_the_plugin_id_from_a_loader_failure() {
        let message = "Error: dsh: plugin tree failed to load: \
failed to apply loader entry include (cordis:include): \
failed to apply loader entry dsh-plugins-catalog (dsh-plugins-catalog): \
invalid plugin, expect function or object with an \"apply\" method, received object";
        assert_eq!(
            failing_loader_entry(message),
            Some("dsh-plugins-catalog".to_string())
        );
        assert_eq!(failing_loader_entry("dsh web 进程已退出 (code 1)"), None);
        assert_eq!(failing_loader_entry("failed to apply loader entry "), None);
    }

    #[test]
    fn rescue_patch_disables_each_named_plugin() {
        assert_eq!(
            rescue_patch_body(&["a-b".to_string(), "c.d".to_string()]),
            "- id: a-b\n  disabled: true\n- id: c.d\n  disabled: true\n"
        );
    }

    #[test]
    fn parses_linux_pid_from_first_stderr_line() {
        assert_eq!(parse_linux_pid_from_stderr("43210\nready\n"), Some(43210));
        assert_eq!(parse_linux_pid_from_stderr("not-a-pid\n"), None);
        assert_eq!(
            parse_linux_pid_from_stderr("wsl: localhost proxy\n43210\nready\n"),
            Some(43210)
        );
    }

    #[test]
    fn stop_argv_is_kill_not_terminate() {
        let args = wsl_stop_args("Ubuntu", 43210);
        assert_eq!(args[0], "-d");
        assert!(args.contains(&"kill".into()));
        assert!(!args.iter().any(|a| a == "--terminate"));
    }

    #[test]
    fn handshake_scans_past_diagnostic_lines() {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let stderr = std::io::Cursor::new(b"wsl: diagnostic\n43210\nafter\n".to_vec());
        let pid =
            read_linux_pid_handshake(stderr, Arc::clone(&lines), Duration::from_secs(2)).unwrap();
        assert_eq!(pid, 43210);
        std::thread::sleep(Duration::from_millis(50));
        let captured = lines.lock().unwrap();
        assert!(captured.iter().any(|l| l.contains("diagnostic")));
        assert!(captured.iter().any(|l| l == "43210"));
        assert!(captured.iter().any(|l| l == "after"));
    }

    #[test]
    fn handshake_times_out_when_stderr_stalls() {
        struct Stall;
        impl Read for Stall {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_secs(10));
                Ok(0)
            }
        }
        let lines = Arc::new(Mutex::new(Vec::new()));
        let err = read_linux_pid_handshake(Stall, lines, Duration::from_millis(200)).unwrap_err();
        assert!(
            err.contains("超时"),
            "expected timeout error, got: {err}"
        );
    }
}
