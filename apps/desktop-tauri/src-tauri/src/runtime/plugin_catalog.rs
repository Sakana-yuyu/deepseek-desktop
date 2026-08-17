//! Tray-driven install of the shipped plugin catalog into the Host `web` profile.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::boot_log;
use super::config::npm_registry;
use super::process::hide_console;
use super::profile_repair::HOST_PROFILE;
use super::wsl::{reject_windows_node, WslCommand, WslRuntimePaths};
use super::DesktopRuntime;
use crate::chrome;
use crate::i18n::{self, Msg};
use crate::notify;

/// pnpm git spec for https://github.com/Sakana-yuyu/dsh-plugins.
pub const CATALOG_SPEC: &str = "github:Sakana-yuyu/dsh-plugins";

const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

static INSTALLING: AtomicBool = AtomicBool::new(false);

/// Where `dsh plugin --profile web add` must run: the live Host's home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginRunTarget {
    /// Windows Node against the desktop `$DSH_HOME`.
    Windows {
        node: PathBuf,
        cli: PathBuf,
        harness_root: PathBuf,
        dsh_home: PathBuf,
        host_path: String,
    },
    /// Linux Node inside the selected WSL2 distro against distro `~/.dsh`.
    Wsl(WslRuntimePaths),
}

/// Tray entry: install the catalog into the running Host profile, then restart.
pub fn begin_from_tray(app: &AppHandle) {
    if INSTALLING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        notify::toast(app, "DeepSeek Harness", i18n::t(Msg::CatalogBusy));
        return;
    }

    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        INSTALLING.store(false, Ordering::SeqCst);
        notify::toast(app, "DeepSeek Harness", i18n::t(Msg::CatalogNotReady));
        return;
    };
    let target = runtime.plugin_target.clone();
    notify::toast(app, "DeepSeek Harness", i18n::t(Msg::CatalogInstalling));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::task::spawn_blocking(move || install_catalog(&target)).await;
        match result {
            Ok(Ok(())) => {
                notify::toast(&app, "DeepSeek Harness", i18n::t(Msg::CatalogRestarting));
                chrome::request_restart(&app);
            }
            Ok(Err(error)) => {
                boot_log::error(&format!("plugin catalog install failed: {error}"));
                notify::toast(&app, i18n::t(Msg::CatalogFailed), &error);
                INSTALLING.store(false, Ordering::SeqCst);
            }
            Err(error) => {
                boot_log::error(&format!("plugin catalog install task failed: {error}"));
                notify::toast(&app, i18n::t(Msg::CatalogFailed), &error.to_string());
                INSTALLING.store(false, Ordering::SeqCst);
            }
        }
    });
}

/// `dsh plugin --profile web add <catalog spec>` argv after the CLI entry.
pub fn plugin_add_argv() -> [&'static str; 5] {
    ["plugin", "--profile", HOST_PROFILE, "add", CATALOG_SPEC]
}

fn install_catalog(target: &PluginRunTarget) -> Result<(), String> {
    let mut cmd = match target {
        PluginRunTarget::Windows {
            node,
            cli,
            harness_root,
            dsh_home,
            host_path,
        } => {
            let mut cmd = Command::new(node);
            cmd.arg(cli)
                .args(plugin_add_argv())
                .current_dir(harness_root)
                .env("DSH_HOME", dsh_home)
                .env("PATH", host_path);
            cmd
        }
        PluginRunTarget::Wsl(paths) => {
            let wsl = wsl_plugin_add_command(paths)?;
            let mut cmd = Command::new(&wsl.program);
            cmd.args(&wsl.args);
            cmd
        }
    };
    cmd.env("NODE_ENV", "production")
        .env("npm_config_registry", npm_registry())
        .env_remove("CI")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    run_with_timeout(cmd)
}

fn wsl_plugin_add_command(paths: &WslRuntimePaths) -> Result<WslCommand, String> {
    reject_windows_node(&paths.linux_node)?;
    let mut args = vec![
        "-d".into(),
        paths.distro.clone(),
        "--cd".into(),
        paths.linux_harness_root.clone(),
        "--exec".into(),
        "/usr/bin/env".into(),
        format!("PATH={}", paths.linux_path),
        format!("DSH_HOME={}", paths.linux_dsh_home),
        "NODE_ENV=production".into(),
        format!("npm_config_registry={}", npm_registry()),
        paths.linux_node.clone(),
        paths.linux_cli.clone(),
    ];
    args.extend(plugin_add_argv().into_iter().map(str::to_string));
    Ok(WslCommand {
        program: "wsl.exe".into(),
        args,
    })
}

fn run_with_timeout(mut cmd: Command) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", i18n::t(Msg::CatalogFailed)))?;
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
                        "{} ({}s){}",
                        i18n::t(Msg::CatalogFailed),
                        INSTALL_TIMEOUT.as_secs(),
                        format_output_tail(&output_tail)
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(error) => return Err(format!("{}: {error}", i18n::t(Msg::CatalogFailed))),
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} (exit {}){}",
            i18n::t(Msg::CatalogFailed),
            status.code().unwrap_or(-1),
            format_output_tail(&output_tail)
        ))
    }
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
    let lines = tail
        .lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{lines}")
    }
}

#[cfg(test)]
mod tests {
    use super::{plugin_add_argv, wsl_plugin_add_command, CATALOG_SPEC};
    use crate::runtime::wsl::WslRuntimePaths;

    fn wsl_paths() -> WslRuntimePaths {
        WslRuntimePaths {
            distro: "Ubuntu".into(),
            linux_node: "/home/u/.local/share/dsh-desktop/runtime/node/bin/node".into(),
            linux_cli: "/home/u/.local/share/dsh-desktop/harness-versions/abc/apps/cli/lib/bin.js"
                .into(),
            linux_harness_root: "/home/u/.local/share/dsh-desktop/harness-versions/abc".into(),
            linux_dsh_home: "/home/u/.dsh".into(),
            linux_path: "/home/u/.local/share/dsh-desktop/runtime/node/bin:/usr/bin".into(),
            linux_patch: None,
        }
    }

    #[test]
    fn add_argv_matches_documented_command() {
        assert_eq!(
            plugin_add_argv(),
            ["plugin", "--profile", "web", "add", CATALOG_SPEC]
        );
        assert_eq!(CATALOG_SPEC, "github:Sakana-yuyu/dsh-plugins");
    }

    #[test]
    fn wsl_argv_runs_linux_node_plugin_add() {
        let cmd = wsl_plugin_add_command(&wsl_paths()).unwrap();
        assert_eq!(cmd.program, "wsl.exe");
        assert!(cmd.args.iter().any(|a| a == "--exec"));
        assert!(!cmd.args.iter().any(|a| a == "bash" || a == "-lc"));
        let join = cmd.args.join(" ");
        assert!(join.contains("plugin --profile web add github:Sakana-yuyu/dsh-plugins"));
        assert!(join.contains("/home/u/.local/share/dsh-desktop/runtime/node/bin/node"));
        assert!(join.contains("DSH_HOME=/home/u/.dsh"));
    }

    #[test]
    fn wsl_argv_rejects_windows_node() {
        let mut paths = wsl_paths();
        paths.linux_node = r"C:\Program Files\nodejs\node.exe".into();
        let err = wsl_plugin_add_command(&paths).unwrap_err();
        assert_eq!(err, "禁止在 WSL 中执行 Windows node.exe");
    }
}
