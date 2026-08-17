//! Provision a Linux harness tree, Node runtime, and `$HOME/.dsh` inside WSL.

use std::fs;
use std::path::Path;

use crate::overlay::{linux_plugin_file_url, overlay_yaml};
use crate::runtime::config::{npm_registry, DEFAULT_NODE_VERSION, DEFAULT_PNPM_VERSION};
use crate::runtime::host_env::node_version_compatible;
use crate::runtime::provision::{download_file, node_archive_spec_for};
use crate::runtime::ProvisionEvent;

use super::{windows_to_wsl_mount, WslOutput, WslRunner};

const ERR_HARNESS: &str = "无法在 WSL 中安装 harness 树";
const ERR_NODE: &str = "无法在 WSL 中安装 Node";
const ERR_PNPM: &str = "无法在 WSL 中执行 pnpm install";
const PNPM_TIMEOUT_SECS: &str = "600";

/// Linux paths for a WSL-mode Host after provisioning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WslRuntimePaths {
    pub distro: String,
    pub linux_node: String,
    pub linux_cli: String,
    pub linux_harness_root: String,
    pub linux_dsh_home: String,
    pub linux_path: String,
    pub linux_patch: Option<String>,
}

/// Copy the bundled tree onto the Linux disk, ensure a Linux Node, run
/// `pnpm install`, seed `$HOME/.dsh` credentials once, and implant the overlay.
pub async fn ensure_wsl_runtime(
    runner: &dyn WslRunner,
    distro: &str,
    bundled_windows: &Path,
    bundle_hash: &str,
    windows_dsh_home: &Path,
    overlay_src: Option<&Path>,
    _notify_url: Option<&str>,
    progress: impl Fn(ProvisionEvent),
) -> Result<WslRuntimePaths, String> {
    progress(ProvisionEvent::Status("正在准备 WSL 运行环境…".into()));

    let linux_home = printenv_home(runner, distro)?;
    let linux_harness_root = format!(
        "{linux_home}/.local/share/dsh-desktop/harness-versions/{bundle_hash}"
    );
    let linux_cli = format!("{linux_harness_root}/apps/cli/lib/bin.js");
    let linux_runtime_root = format!("{linux_home}/.local/share/dsh-desktop/runtime");
    let preferred_node = format!("{linux_runtime_root}/node/bin/node");
    let linux_dsh_home = format!("{linux_home}/.dsh");

    ensure_harness_tree(runner, distro, bundled_windows, &linux_harness_root, &linux_cli)?;
    progress(ProvisionEvent::Progress(20));

    let linux_node =
        ensure_linux_node(runner, distro, &preferred_node, &linux_runtime_root, &progress).await?;
    progress(ProvisionEvent::Progress(45));

    let node_bin_dir = linux_node
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .ok_or_else(|| format!("{ERR_NODE}: invalid node path {linux_node}"))?;
    let linux_path = format!("{node_bin_dir}:/usr/bin");

    run_pnpm_install(runner, distro, &linux_path, &linux_harness_root)?;
    progress(ProvisionEvent::Progress(75));

    seed_linux_home(runner, distro, windows_dsh_home, &linux_dsh_home)?;

    let linux_patch = if let Some(src) = overlay_src {
        Some(implant_overlay(runner, distro, src, &linux_dsh_home)?)
    } else {
        None
    };
    progress(ProvisionEvent::Progress(90));

    Ok(WslRuntimePaths {
        distro: distro.to_string(),
        linux_node,
        linux_cli,
        linux_harness_root,
        linux_dsh_home,
        linux_path,
        linux_patch,
    })
}

fn printenv_home(runner: &dyn WslRunner, distro: &str) -> Result<String, String> {
    let out = wsl_exec(runner, distro, &["printenv", "HOME"])
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    require_success(&out, ERR_HARNESS)?;
    let home = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if home.is_empty() || !home.starts_with('/') {
        return Err(format!("{ERR_HARNESS}: empty or invalid HOME"));
    }
    Ok(home)
}

fn ensure_harness_tree(
    runner: &dyn WslRunner,
    distro: &str,
    bundled_windows: &Path,
    linux_harness_root: &str,
    linux_cli: &str,
) -> Result<(), String> {
    if wsl_test_f(runner, distro, linux_cli)? {
        return Ok(());
    }

    let src = windows_to_wsl_mount(bundled_windows)
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    let mkdir = wsl_exec(runner, distro, &["mkdir", "-p", linux_harness_root])
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    require_success(&mkdir, ERR_HARNESS)?;

    let src_dot = format!("{src}/.");
    let dest_slash = format!("{linux_harness_root}/");
    let copy = wsl_exec(runner, distro, &["cp", "-a", &src_dot, &dest_slash])
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    require_success(&copy, ERR_HARNESS)?;
    Ok(())
}

async fn ensure_linux_node(
    runner: &dyn WslRunner,
    distro: &str,
    preferred_node: &str,
    linux_runtime_root: &str,
    progress: &impl Fn(ProvisionEvent),
) -> Result<String, String> {
    if let Some(node) = probe_compatible_node(runner, distro, preferred_node)? {
        return Ok(node);
    }

    let which = wsl_exec(runner, distro, &["/bin/sh", "-c", "command -v node"])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    if which.code == 0 {
        let path = String::from_utf8_lossy(&which.stdout).trim().to_string();
        if !path.is_empty() {
            if let Some(node) = probe_compatible_node(runner, distro, &path)? {
                return Ok(node);
            }
        }
    }

    install_linux_node(runner, distro, preferred_node, linux_runtime_root, progress).await
}

fn probe_compatible_node(
    runner: &dyn WslRunner,
    distro: &str,
    node_path: &str,
) -> Result<Option<String>, String> {
    if node_path != "node" {
        let exists = wsl_exec(runner, distro, &["test", "-x", node_path])
            .map_err(|e| format!("{ERR_NODE}: {e}"))?;
        if exists.code != 0 {
            return Ok(None);
        }
    }

    let ver = wsl_exec(runner, distro, &[node_path, "-v"])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    if ver.code != 0 {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&ver.stdout);
    let line = raw.lines().next().unwrap_or("").trim();
    if node_version_compatible(line) {
        Ok(Some(node_path.to_string()))
    } else {
        Ok(None)
    }
}

async fn install_linux_node(
    runner: &dyn WslRunner,
    distro: &str,
    preferred_node: &str,
    linux_runtime_root: &str,
    progress: &impl Fn(ProvisionEvent),
) -> Result<String, String> {
    let arch = linux_arch(runner, distro)?;
    let spec = node_archive_spec_for(DEFAULT_NODE_VERSION, "linux", &arch)
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;

    let cache = crate::runtime::app_data_root()
        .map_err(|e| format!("{ERR_NODE}: {e}"))?
        .join("cache");
    fs::create_dir_all(&cache).map_err(|e| format!("{ERR_NODE}: {e}"))?;
    let archive_path = cache.join(&spec.archive_name);
    if !archive_path.is_file() {
        progress(ProvisionEvent::Status(format!(
            "正在下载 Linux Node {DEFAULT_NODE_VERSION}…"
        )));
        download_file(&spec.url, &archive_path, 30, 40, progress)
            .await
            .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    }

    let archive_mnt = windows_to_wsl_mount(&archive_path)
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    let node_dir = format!("{linux_runtime_root}/node");

    let rm = wsl_exec(runner, distro, &["rm", "-rf", &node_dir])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    require_success(&rm, ERR_NODE)?;

    let mkdir = wsl_exec(runner, distro, &["mkdir", "-p", &node_dir])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    require_success(&mkdir, ERR_NODE)?;

    let tar = wsl_exec(
        runner,
        distro,
        &[
            "tar",
            "-xzf",
            &archive_mnt,
            "--strip-components=1",
            "-C",
            &node_dir,
        ],
    )
    .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    require_success(&tar, ERR_NODE)?;

    let check = wsl_exec(runner, distro, &["test", "-x", preferred_node])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    require_success(&check, ERR_NODE)?;
    Ok(preferred_node.to_string())
}

fn linux_arch(runner: &dyn WslRunner, distro: &str) -> Result<String, String> {
    let out = wsl_exec(runner, distro, &["uname", "-m"])
        .map_err(|e| format!("{ERR_NODE}: {e}"))?;
    require_success(&out, ERR_NODE)?;
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match raw.as_str() {
        "x86_64" => Ok("x86_64".into()),
        "aarch64" | "arm64" => Ok("aarch64".into()),
        other => Err(format!("{ERR_NODE}: unsupported uname -m: {other}")),
    }
}

fn run_pnpm_install(
    runner: &dyn WslRunner,
    distro: &str,
    linux_path: &str,
    linux_harness_root: &str,
) -> Result<(), String> {
    let registry = npm_registry();
    let script = format!(
        "cd \"$1\" && corepack enable && corepack prepare pnpm@{pnpm} --activate && pnpm install --prod --no-frozen-lockfile --registry \"$2\"",
        pnpm = DEFAULT_PNPM_VERSION,
    );
    let path_env = format!("PATH={linux_path}");
    let out = wsl_exec(
        runner,
        distro,
        &[
            "timeout",
            PNPM_TIMEOUT_SECS,
            "/usr/bin/env",
            "-u",
            "CI",
            &path_env,
            "/bin/sh",
            "-c",
            &script,
            "pnpm-install",
            linux_harness_root,
            &registry,
        ],
    )
    .map_err(|e| format!("{ERR_PNPM}: {e}"))?;
    require_success(&out, ERR_PNPM)?;
    Ok(())
}

fn seed_linux_home(
    runner: &dyn WslRunner,
    distro: &str,
    windows_dsh_home: &Path,
    linux_dsh_home: &str,
) -> Result<(), String> {
    let cred_dest = format!("{linux_dsh_home}/.credentials.yaml");
    let env_dest = format!("{linux_dsh_home}/.env");
    let cred_present = wsl_test_f(runner, distro, &cred_dest)?;
    let env_present = wsl_test_f(runner, distro, &env_dest)?;
    if cred_present || env_present {
        return Ok(());
    }

    let mkdir = wsl_exec(runner, distro, &["mkdir", "-p", linux_dsh_home])
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    require_success(&mkdir, ERR_HARNESS)?;

    let win_cred = windows_dsh_home.join(".credentials.yaml");
    if win_cred.is_file() {
        let src = windows_to_wsl_mount(&win_cred)
            .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
        let copy = wsl_exec(runner, distro, &["cp", &src, &cred_dest])
            .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
        require_success(&copy, ERR_HARNESS)?;
    }

    let win_env = windows_dsh_home.join(".env");
    if win_env.is_file() {
        let src = windows_to_wsl_mount(&win_env)
            .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
        let copy = wsl_exec(runner, distro, &["cp", &src, &env_dest])
            .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
        require_success(&copy, ERR_HARNESS)?;
    }

    Ok(())
}

fn implant_overlay(
    runner: &dyn WslRunner,
    distro: &str,
    overlay_src: &Path,
    linux_dsh_home: &str,
) -> Result<String, String> {
    let plugin_src = overlay_src.join("index.mjs");
    if !plugin_src.is_file() {
        return Err(format!(
            "desktop overlay plugin missing: {}",
            plugin_src.display()
        ));
    }

    let dest_dir = format!("{linux_dsh_home}/desktop-overlay");
    let plugin_dest = format!("{dest_dir}/index.mjs");
    let patch_dest = format!("{dest_dir}/cordis.yml");
    let plugin_url = linux_plugin_file_url(&plugin_dest)?;
    let yaml = overlay_yaml(&plugin_url);

    let mkdir = wsl_exec(runner, distro, &["mkdir", "-p", &dest_dir])
        .map_err(|e| e.to_string())?;
    require_success(&mkdir, "无法写入 WSL overlay")?;

    let src_mnt = windows_to_wsl_mount(&plugin_src)?;
    let copy = wsl_exec(runner, distro, &["cp", &src_mnt, &plugin_dest])
        .map_err(|e| e.to_string())?;
    require_success(&copy, "无法写入 WSL overlay")?;

    let cache = crate::runtime::app_data_root()?.join("cache");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let yaml_path = cache.join("wsl-desktop-overlay-cordis.yml");
    fs::write(&yaml_path, yaml).map_err(|e| e.to_string())?;
    let yaml_mnt = windows_to_wsl_mount(&yaml_path)?;
    let copy_yaml = wsl_exec(runner, distro, &["cp", &yaml_mnt, &patch_dest])
        .map_err(|e| e.to_string())?;
    require_success(&copy_yaml, "无法写入 WSL overlay")?;

    Ok(patch_dest)
}

fn wsl_test_f(runner: &dyn WslRunner, distro: &str, path: &str) -> Result<bool, String> {
    let out = wsl_exec(runner, distro, &["test", "-f", path])
        .map_err(|e| format!("{ERR_HARNESS}: {e}"))?;
    Ok(out.code == 0)
}

fn wsl_exec(runner: &dyn WslRunner, distro: &str, program_args: &[&str]) -> Result<WslOutput, String> {
    let mut args: Vec<&str> = Vec::with_capacity(3 + program_args.len());
    args.push("-d");
    args.push(distro);
    args.push("--exec");
    args.extend_from_slice(program_args);
    runner.run(&args)
}

fn require_success(out: &WslOutput, prefix: &str) -> Result<(), String> {
    if out.code == 0 {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else if !stdout.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        format!("exit {}", out.code)
    };
    Err(format!("{prefix}: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::ensure_wsl_runtime;
    use crate::runtime::wsl::{WslOutput, WslRunner};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// Fake runner: scripted replies keyed by args prefix; records every invocation.
    struct Scripted {
        scripts: Mutex<Vec<(Vec<String>, WslOutput)>>,
        recorded: Mutex<Vec<Vec<String>>>,
    }

    impl Scripted {
        fn new(scripts: Vec<(Vec<String>, WslOutput)>) -> Self {
            Self {
                scripts: Mutex::new(scripts),
                recorded: Mutex::new(Vec::new()),
            }
        }

        fn recorded(&self) -> Vec<Vec<String>> {
            self.recorded.lock().expect("recorded lock").clone()
        }
    }

    impl WslRunner for Scripted {
        fn run(&self, args: &[&str]) -> Result<WslOutput, String> {
            let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
            self.recorded
                .lock()
                .expect("recorded lock")
                .push(owned.clone());

            let mut scripts = self.scripts.lock().expect("scripts lock");
            let idx = scripts.iter().position(|(prefix, _)| {
                owned.len() >= prefix.len() && owned.iter().zip(prefix.iter()).all(|(a, b)| a == b)
            });
            match idx {
                Some(i) => Ok(scripts.remove(i).1),
                None => Err(format!("unexpected wsl args: {owned:?}")),
            }
        }
    }

    fn ok_out(stdout: &str) -> WslOutput {
        WslOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            code: 0,
        }
    }

    fn fail_out() -> WslOutput {
        WslOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            code: 1,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-wsl-provision-{}-{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_credentials_once_and_skips_sessions() {
        let bundled = temp_dir("bundled");
        let windows_home = temp_dir("win-home");
        fs::write(windows_home.join(".credentials.yaml"), "token: test\n").unwrap();
        fs::write(windows_home.join(".env"), "KEY=1\n").unwrap();
        fs::create_dir_all(windows_home.join("sessions")).unwrap();
        fs::write(windows_home.join("sessions").join("a.json"), "{}").unwrap();

        let hash = "abc123";
        let harness_bin = format!(
            "/home/u/.local/share/dsh-desktop/harness-versions/{hash}/apps/cli/lib/bin.js"
        );
        let preferred_node = "/home/u/.local/share/dsh-desktop/runtime/node/bin/node";
        let cred_dest = "/home/u/.dsh/.credentials.yaml";
        let env_dest = "/home/u/.dsh/.env";

        let runner = Scripted::new(vec![
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "printenv".into(),
                    "HOME".into(),
                ],
                ok_out("/home/u\n"),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "test".into(),
                    "-f".into(),
                    harness_bin,
                ],
                fail_out(),
            ),
            (
                vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "mkdir".into()],
                ok_out(""),
            ),
            (
                vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "cp".into()],
                ok_out(""),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "test".into(),
                    "-x".into(),
                    preferred_node.into(),
                ],
                fail_out(),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "/bin/sh".into(),
                    "-c".into(),
                    "command -v node".into(),
                ],
                ok_out("/usr/bin/node\n"),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "test".into(),
                    "-x".into(),
                    "/usr/bin/node".into(),
                ],
                ok_out(""),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "/usr/bin/node".into(),
                    "-v".into(),
                ],
                ok_out("v22.19.0\n"),
            ),
            (
                vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "timeout".into()],
                ok_out(""),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "test".into(),
                    "-f".into(),
                    cred_dest.into(),
                ],
                fail_out(),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "test".into(),
                    "-f".into(),
                    env_dest.into(),
                ],
                fail_out(),
            ),
            (
                vec![
                    "-d".into(),
                    "Ubuntu".into(),
                    "--exec".into(),
                    "mkdir".into(),
                    "-p".into(),
                    "/home/u/.dsh".into(),
                ],
                ok_out(""),
            ),
            (
                vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "cp".into()],
                ok_out(""),
            ),
            (
                vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "cp".into()],
                ok_out(""),
            ),
        ]);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let paths = rt
            .block_on(ensure_wsl_runtime(
                &runner,
                "Ubuntu",
                &bundled,
                hash,
                &windows_home,
                None,
                None,
                |_| {},
            ))
            .expect("ensure_wsl_runtime");

        assert_eq!(paths.linux_dsh_home, "/home/u/.dsh");
        assert_eq!(
            paths.linux_harness_root,
            format!("/home/u/.local/share/dsh-desktop/harness-versions/{hash}")
        );
        assert_eq!(paths.linux_node, "/usr/bin/node");
        assert!(paths.linux_node.ends_with("/bin/node"));
        assert!(!paths.linux_node.to_ascii_lowercase().ends_with("node.exe"));

        let recorded = runner.recorded();
        let cp_args: Vec<&Vec<String>> = recorded
            .iter()
            .filter(|args| args.iter().any(|a| a == "cp"))
            .collect();

        let credentials_cp = cp_args
            .iter()
            .find(|args| args.iter().any(|a| a.contains(".credentials.yaml")));
        assert!(
            credentials_cp.is_some(),
            "expected a cp of .credentials.yaml, recorded: {recorded:?}"
        );
        let credentials_cp = credentials_cp.unwrap();
        assert!(
            credentials_cp
                .iter()
                .any(|a| a.starts_with("/mnt/") && a.contains(".credentials.yaml")),
            "credentials cp source must be under /mnt/: {credentials_cp:?}"
        );
        assert!(
            credentials_cp
                .iter()
                .any(|a| a == cred_dest || a.ends_with("/.dsh/.credentials.yaml")),
            "credentials cp dest under /home/u/.dsh: {credentials_cp:?}"
        );

        for args in &cp_args {
            assert!(
                args.iter().all(|a| !a.contains("sessions")),
                "cp must never copy sessions/: {args:?}"
            );
        }

        let harness_cp = cp_args.iter().find(|args| {
            args.iter()
                .any(|a| a.contains("dsh-desktop/harness-versions"))
        });
        assert!(
            harness_cp.is_some(),
            "expected harness tree cp into ~/.local/share/dsh-desktop/, recorded: {recorded:?}"
        );
        let harness_cp = harness_cp.unwrap();
        assert!(
            harness_cp
                .iter()
                .any(|a| a.starts_with("/mnt/") && !a.contains(".credentials")),
            "harness cp source must be under /mnt/: {harness_cp:?}"
        );

        let _ = fs::remove_dir_all(&bundled);
        let _ = fs::remove_dir_all(&windows_home);
        let _ = Path::new("");
    }
}
