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

/// Maximum broken plugins one boot disables before giving up on the Host.
const MAX_PLUGIN_RESCUES: usize = 4;

/// Running `dsh web` child, bound port, and verified base URL.
pub struct HostHandle {
    pub port: u16,
    pub web_url: String,
    /// Plugin entry ids whose load failure was bypassed through a rescue
    /// `--patch` this session; empty when the Host started clean.
    pub disabled_plugins: Vec<String>,
    child: Arc<Mutex<Option<Child>>>,
    #[cfg(windows)]
    job: Mutex<Option<super::process::KillOnCloseJob>>,
}

impl HostHandle {
    /// Stop the Host Node tree. Safe to call more than once, including before
    /// `app.exit` / `app.restart`, which do not run `Drop`.
    pub fn stop(&self) {
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
            child: child_handle,
            #[cfg(windows)]
            job: Mutex::new(job),
        });
    }
    Err(last_error)
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
            return Err(format!("等待 {url} 就绪超时"));
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
    use super::{failing_loader_entry, rescue_patch_body};

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
}
