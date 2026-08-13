use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::boot_log;
use super::config::DEFAULT_WEB_PORT;
use super::process::hide_console;
use super::provision::RuntimePaths;

/// Running `dsh web` child, bound port, and verified base URL.
pub struct HostHandle {
    pub port: u16,
    pub web_url: String,
    child: Arc<Mutex<Option<Child>>>,
}

impl Drop for HostHandle {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Spawn `dsh web --host 127.0.0.1 --port <port>` and wait until HTTP responds.
pub async fn spawn_web_host(paths: &RuntimePaths) -> Result<HostHandle, String> {
    if !paths.cli_entry.is_file() {
        return Err(format!(
            "harness CLI 缺失: {} — 请确认安装包内已包含 apps/cli/lib",
            paths.cli_entry.display()
        ));
    }

    let port = pick_port(DEFAULT_WEB_PORT)?;
    let web_url = format!("http://127.0.0.1:{port}/");
    boot_log::info(&format!(
        "spawning dsh web node={} cli={} port={port}",
        paths.node_binary.display(),
        paths.cli_entry.display()
    ));
    let child = spawn_child(paths, port)?;
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

    wait_for_http(
        &web_url,
        &child_handle,
        &stderr_lines,
        Duration::from_secs(120),
    )
    .await?;
    boot_log::info(&format!("health check passed url={web_url}"));

    Ok(HostHandle {
        port,
        web_url,
        child: child_handle,
    })
}

fn spawn_child(paths: &RuntimePaths, port: u16) -> Result<Child, String> {
    let mut cmd = Command::new(&paths.node_binary);
    cmd.arg(&paths.cli_entry)
        .arg("web")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .env("DSH_HOME", &paths.dsh_home)
        .env("NODE_ENV", "production")
        .current_dir(&paths.harness_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

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
    let mut consecutive_ok = 0u8;

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
                consecutive_ok += 1;
                if consecutive_ok >= 3 {
                    boot_log::info(&format!(
                        "http ready status={} url={url}",
                        response.status()
                    ));
                    return Ok(());
                }
            }
            Ok(response) => {
                boot_log::info(&format!(
                    "health probe non-success status={} url={url}",
                    response.status()
                ));
                consecutive_ok = 0;
            }
            Err(err) => {
                if consecutive_ok == 0 {
                    boot_log::info(&format!("health probe failed url={url} err={err}"));
                }
                consecutive_ok = 0;
            }
        }

        tokio::time::sleep(Duration::from_millis(400)).await;
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
