//! Reinstall broken or missing profile dependencies before the Host starts.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::boot_log;
use super::process::hide_console;
use super::provision::RuntimePaths;
use super::user_home::{profile_dependencies_unresolved, profiles_needing_install};
use super::ProvisionEvent;

/// How long one `dsh plugin --profile <name> install` may run before it is killed.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// The profile `dsh web` boots; a profile whose install cannot be repaired is
/// a boot error only for this one, other profiles defer to a later `dsh plugin`.
pub const HOST_PROFILE: &str = "web";

/// Ensure every profile under `DSH_HOME` can resolve its declared dependencies
/// before the Host starts: profiles needing install run
/// `dsh plugin --profile <name> install` (which forwards to pnpm on the bridged
/// PATH) and are re-verified afterwards. A failed repair of a non-Host profile
/// is logged and deferred; a failed repair of {@link HOST_PROFILE} fails boot
/// with the manual command to run.
pub async fn ensure_profile_installs(
    paths: &RuntimePaths,
    host_path: &str,
    progress: &Arc<dyn Fn(ProvisionEvent) + Send + Sync>,
) -> Result<(), String> {
    let pending = profiles_needing_install(&paths.dsh_home);
    if pending.is_empty() {
        return Ok(());
    }
    boot_log::info(&format!("profile installs pending: {}", pending.join(", ")));
    for name in pending {
        progress(ProvisionEvent::Status(format!("正在安装 profile {name} 依赖…")));
        let paths = paths.clone();
        let host_path = host_path.to_string();
        let name_for_task = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            run_profile_install(&paths, &host_path, &name_for_task)
        })
        .await
        .map_err(|e| format!("profile {name} 安装任务失败: {e}"))?;
        match result {
            Ok(()) => {
                progress(ProvisionEvent::Status(format!("profile {name} 依赖已就绪")));
                boot_log::info(&format!("profile {name} dependencies installed"));
            }
            Err(error) => {
                boot_log::error(&format!("profile {name} install failed: {error}"));
                if name == HOST_PROFILE {
                    return Err(format!(
                        "profile {name} 依赖安装失败: {error}\n请检查网络后重试，或手动运行 dsh plugin --profile {name} install"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Run one install and re-verify the profile can resolve its dependencies.
fn run_profile_install(paths: &RuntimePaths, host_path: &str, name: &str) -> Result<(), String> {
    let mut cmd = Command::new(&paths.node_binary);
    cmd.arg(&paths.cli_entry)
        .arg("plugin")
        .arg("--profile")
        .arg(name)
        .arg("install")
        .env("DSH_HOME", &paths.dsh_home)
        .env("PATH", host_path)
        .env("NODE_ENV", "production")
        .current_dir(&paths.harness_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 dsh plugin: {e}"))?;

    let output_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    if let Some(stdout) = child.stdout.take() {
        let tail = Arc::clone(&output_tail);
        std::thread::spawn(move || drain_lines(stdout, tail));
    }
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&output_tail);
        std::thread::spawn(move || drain_lines(stderr, tail));
    }

    let deadline = Instant::now() + INSTALL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "安装超时（超过 {} 秒）{}",
                        INSTALL_TIMEOUT.as_secs(),
                        format_output_tail(&output_tail)
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(error) => return Err(format!("等待安装进程失败: {error}")),
        }
    };

    if !status.success() {
        return Err(format!(
            "pnpm 退出码 {}{}",
            status.code().unwrap_or(-1),
            format_output_tail(&output_tail)
        ));
    }
    let profile_dir = profile_dir(&paths.dsh_home, name);
    if profile_dependencies_unresolved(&profile_dir) {
        return Err(format!(
            "安装完成但依赖仍无法解析{}",
            format_output_tail(&output_tail)
        ));
    }
    Ok(())
}

fn profile_dir(dsh_home: &std::path::Path, name: &str) -> PathBuf {
    dsh_home.join("profiles").join(name)
}

fn drain_lines<R: std::io::Read>(reader: R, sink: Arc<Mutex<Vec<String>>>) {
    let reader = BufReader::new(reader);
    for line in reader.lines().flatten() {
        if let Ok(mut guard) = sink.lock() {
            guard.push(line);
            if guard.len() > 20 {
                let drop = guard.len() - 20;
                guard.drain(0..drop);
            }
        }
    }
}

fn format_output_tail(tail: &Arc<Mutex<Vec<String>>>) -> String {
    let lines = tail.lock().map(|lines| lines.join("\n")).unwrap_or_default();
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{lines}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::user_home::profiles_needing_install;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "dsh-desktop-repair-{}-{}-{}",
            std::process::id(),
            nanos,
            id
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn lists_profiles_with_unresolved_dependencies_only() {
        let root = temp_root();
        let home = root.join("home");
        let broken = home.join("profiles").join("web");
        let intact = home.join("profiles").join("cli");
        let plain = home.join("profiles").join("plain");
        for dir in [&broken, &intact, &plain] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(
            broken.join("package.json"),
            r#"{"dependencies":{"dsh-plugins-catalog":"github:x/y"}}"#,
        )
        .unwrap();
        fs::write(
            intact.join("package.json"),
            r#"{"dependencies":{"kept":"1.0.0"}}"#,
        )
        .unwrap();
        fs::create_dir_all(intact.join("node_modules").join("kept")).unwrap();
        fs::write(
            intact.join("node_modules").join("kept").join("package.json"),
            r#"{"name":"kept"}"#,
        )
        .unwrap();
        fs::write(plain.join("package.json"), r#"{"dependencies":{}}"#).unwrap();

        assert_eq!(profiles_needing_install(&home), vec!["web".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_home_needs_no_installs() {
        let root = temp_root();
        assert!(profiles_needing_install(&root.join("home")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
