//! Adopt an existing Harness home so CLI sessions and keys reach the desktop Host.

use std::fs;
use std::path::{Path, PathBuf};

use super::boot_log;
use super::env_path::{durable_dsh_home, path_eq};

const HOME_DIR_NAME: &str = ".dsh";
const SKIP_IMPORT: &[&str] = &["desktop-overlay", "node_modules"];
const HOME_MARKERS: &[&str] = &[
    "sessions",
    ".credentials.yaml",
    ".env",
    "profiles",
    "settings.yaml",
    "settings.yml",
    "settings.json",
];

/// Selected Host home plus how many missing entries were imported into it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUserHome {
    pub path: PathBuf,
    pub imported: usize,
}

/// Pick `$DSH_HOME` or `~/.dsh` when they already hold Harness data, then
/// copy missing sessions, credentials, and settings from every other home.
/// Access-denied or missing-path failures fall back to the isolated home.
pub fn resolve_user_home(isolated: &Path) -> ResolvedUserHome {
    match adopt_homes(isolated, discover_harness_homes(isolated)) {
        Ok(resolved) => resolved,
        Err(error) => {
            boot_log::info(&format!(
                "home adopt fallback to {}: {error}",
                isolated.display()
            ));
            let _ = fs::create_dir_all(isolated);
            scrub_profile_node_modules(isolated);
            ResolvedUserHome {
                path: isolated.to_path_buf(),
                imported: 0,
            }
        }
    }
}

fn adopt_homes(isolated: &Path, homes: Vec<PathBuf>) -> Result<ResolvedUserHome, String> {
    let selected = homes
        .first()
        .cloned()
        .unwrap_or_else(|| isolated.to_path_buf());
    fs::create_dir_all(&selected).map_err(|e| format!("无法创建 {}: {e}", selected.display()))?;
    scrub_profile_node_modules(&selected);
    for home in &homes {
        if !path_eq(home, &selected) {
            scrub_profile_node_modules(home);
        }
    }

    let mut imported = 0usize;
    for source in &homes {
        if path_eq(source, &selected) {
            continue;
        }
        imported += match import_missing(source, &selected) {
            Ok(count) => count,
            Err(error) => {
                boot_log::info(&format!(
                    "home import skipped {} -> {}: {error}",
                    source.display(),
                    selected.display()
                ));
                0
            }
        };
    }

    boot_log::info(&format!(
        "dsh home selected={} imported={imported} candidates={}",
        selected.display(),
        homes.len()
    ));
    Ok(ResolvedUserHome {
        path: selected,
        imported,
    })
}

/// Splash line after home matching.
pub fn user_home_status(resolved: &ResolvedUserHome, isolated: &Path) -> String {
    if resolved.path == isolated && resolved.imported == 0 {
        "未发现已有对话，将使用桌面端主目录".into()
    } else if resolved.imported == 0 {
        format!("已匹配已有主目录 {}", display_home(&resolved.path))
    } else {
        format!(
            "已恢复 {} 项历史数据到 {}",
            resolved.imported,
            display_home(&resolved.path)
        )
    }
}

/// True when `path` looks like a Harness home rather than an empty folder.
pub fn is_harness_home(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    HOME_MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Copy `from` into `to` without replacing files that already exist.
pub fn import_missing(from: &Path, to: &Path) -> Result<usize, String> {
    if !from.is_dir() || path_eq(from, to) {
        return Ok(0);
    }
    fs::create_dir_all(to).map_err(|e| format!("无法创建 {}: {e}", to.display()))?;
    let mut copied = 0usize;
    for entry in fs::read_dir(from).map_err(|e| format!("无法读取 {}: {e}", from.display()))? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                boot_log::info(&format!("home import skip {}: {error}", from.display()));
                continue;
            }
        };
        let name = entry.file_name();
        if should_skip_import(&name) {
            continue;
        }
        match import_entry(&entry.path(), &to.join(&name)) {
            Ok(count) => copied += count,
            Err(error) => boot_log::info(&format!(
                "home import skip {}: {error}",
                entry.path().display()
            )),
        }
    }
    Ok(copied)
}

fn discover_harness_homes(isolated: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    push_unique(&mut homes, env_dsh_home());
    push_unique(&mut homes, default_cli_home());
    if is_harness_home(isolated) {
        push_unique(&mut homes, Some(isolated.to_path_buf()));
    }
    homes
        .into_iter()
        .filter(|path| is_harness_home(path))
        .collect()
}

fn env_dsh_home() -> Option<PathBuf> {
    durable_dsh_home()
}

fn default_cli_home() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(HOME_DIR_NAME))
}

fn push_unique(homes: &mut Vec<PathBuf>, candidate: Option<PathBuf>) {
    let Some(path) = candidate else {
        return;
    };
    if !homes.iter().any(|existing| path_eq(existing, &path)) {
        homes.push(path);
    }
}

fn should_skip_import(name: &std::ffi::OsStr) -> bool {
    SKIP_IMPORT.iter().any(|skip| name == *skip)
}

/// Delete `profiles/node_modules` and broken `profiles/*/node_modules` without
/// following junctions. A profile's `node_modules` is broken when a dependency
/// its `package.json` declares cannot be resolved to a package directory; an
/// intact install is left in place for `dsh` to reuse, so routine Host restarts
/// do not force every profile back through `dsh plugin install`.
fn scrub_profile_node_modules(home: &Path) {
    let profiles = home.join("profiles");
    remove_profile_node_modules(&profiles.join("node_modules"));
    let Ok(entries) = fs::read_dir(&profiles) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if is_reparse_or_symlink(&child) {
            continue;
        }
        if child.is_dir() && profile_install_broken(&child) {
            remove_profile_node_modules(&child.join("node_modules"));
        }
    }
}

/// True when a profile directory declares dependencies that its `node_modules`
/// cannot resolve: a declared package without a readable manifest, or a
/// dangling pnpm junction. `exists()` follows reparse points, so a junction
/// into a removed harness or store tree reads as missing. A profile without
/// `node_modules`, or without dependencies, is never broken.
fn profile_install_broken(profile: &Path) -> bool {
    let node_modules = profile.join("node_modules");
    if fs::symlink_metadata(&node_modules).is_err() {
        return false;
    }
    let Ok(raw) = fs::read_to_string(profile.join("package.json")) else {
        return true;
    };
    if serde_json::from_str::<serde_json::Value>(&raw).is_err() {
        return true;
    }
    profile_dependencies_unresolved(profile)
}

/// True when a profile declares at least one dependency that cannot be
/// resolved under its `node_modules` — the install-is-needed view that, unlike
/// {@link profile_install_broken}, also covers a profile whose `node_modules`
/// is absent altogether.
pub fn profile_dependencies_unresolved(profile: &Path) -> bool {
    let node_modules = profile.join("node_modules");
    let Ok(raw) = fs::read_to_string(profile.join("package.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let Some(dependencies) = manifest.get("dependencies").and_then(|value| value.as_object())
    else {
        return false;
    };
    dependencies
        .keys()
        .any(|name| !node_modules.join(name).join("package.json").exists())
}

/// Profile names under `home/profiles` whose declared dependencies cannot be
/// resolved — the set `dsh plugin --profile <name> install` must repair.
pub fn profiles_needing_install(home: &Path) -> Vec<String> {
    let profiles = home.join("profiles");
    let Ok(entries) = fs::read_dir(&profiles) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let child = entry.path();
        if is_reparse_or_symlink(&child) || !child.is_dir() {
            continue;
        }
        if profile_dependencies_unresolved(&child) {
            if let Some(name) = entry.file_name().into_string().ok() {
                names.push(name);
            }
        }
    }
    names
}

fn remove_profile_node_modules(path: &Path) {
    if !path.exists() && fs::symlink_metadata(path).is_err() {
        return;
    }
    match remove_link_tree(path) {
        Ok(()) => boot_log::info(&format!("removed leftover {}", path.display())),
        Err(error) => boot_log::info(&format!("leftover skip {}: {error}", path.display())),
    }
}

/// Remove a directory tree. Reparse points and symlinks are unlinked, not followed.
fn remove_link_tree(path: &Path) -> Result<(), String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("无法读取 {}: {error}", path.display())),
    };
    if is_reparse_meta(&meta) {
        return unlink_reparse(path);
    }
    if meta.is_file() {
        return fs::remove_file(path).map_err(|e| format!("无法删除 {}: {e}", path.display()));
    }
    for entry in fs::read_dir(path).map_err(|e| format!("无法读取 {}: {e}", path.display()))? {
        let entry = entry.map_err(|e| format!("无法读取 {}: {e}", path.display()))?;
        remove_link_tree(&entry.path())?;
    }
    fs::remove_dir(path).map_err(|e| format!("无法删除 {}: {e}", path.display()))
}

fn is_reparse_meta(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn unlink_reparse(path: &Path) -> Result<(), String> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(_) => fs::remove_file(path).map_err(|e| format!("无法删除链接 {}: {e}", path.display())),
    }
}

fn is_reparse_or_symlink(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn import_entry(from: &Path, to: &Path) -> Result<usize, String> {
    if should_skip_import(from.file_name().unwrap_or_default()) {
        return Ok(0);
    }
    if is_reparse_or_symlink(from) {
        boot_log::info(&format!("home import skip reparse {}", from.display()));
        return Ok(0);
    }
    if from.is_dir() {
        if to.is_file() {
            return Ok(0);
        }
        if !to.exists() {
            copy_tree(from, to)?;
            return Ok(1);
        }
        let mut copied = 0usize;
        for entry in fs::read_dir(from).map_err(|e| format!("无法读取 {}: {e}", from.display()))? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    boot_log::info(&format!("home import skip {}: {error}", from.display()));
                    continue;
                }
            };
            match import_entry(&entry.path(), &to.join(entry.file_name())) {
                Ok(count) => copied += count,
                Err(error) => boot_log::info(&format!(
                    "home import skip {}: {error}",
                    entry.path().display()
                )),
            }
        }
        return Ok(copied);
    }

    if to.exists() {
        return Ok(0);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 {}: {e}", parent.display()))?;
    }
    fs::copy(from, to).map_err(|e| format!("无法复制 {} -> {}: {e}", from.display(), to.display()))?;
    Ok(1)
}

fn copy_tree(source: &Path, dest: &Path) -> Result<(), String> {
    if should_skip_import(source.file_name().unwrap_or_default()) || is_reparse_or_symlink(source) {
        return Ok(());
    }
    if source.is_file() {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建 {}: {e}", parent.display()))?;
        }
        fs::copy(source, dest)
            .map_err(|e| format!("无法复制 {} -> {}: {e}", source.display(), dest.display()))?;
        return Ok(());
    }

    fs::create_dir_all(dest).map_err(|e| format!("无法创建 {}: {e}", dest.display()))?;
    for entry in fs::read_dir(source).map_err(|e| format!("无法读取 {}: {e}", source.display()))? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                boot_log::info(&format!("home import skip {}: {error}", source.display()));
                continue;
            }
        };
        copy_tree(&entry.path(), &dest.join(entry.file_name()))?;
    }
    Ok(())
}

fn display_home(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if path_eq(path, &home.join(HOME_DIR_NAME)) {
            return format!("~/{HOME_DIR_NAME}");
        }
    }
    if let Some(configured) = durable_dsh_home() {
        if path_eq(path, &configured) {
            return "$DSH_HOME".into();
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{adopt_homes, import_missing, is_harness_home, user_home_status};
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
            "dsh-desktop-home-{}-{}-{}",
            std::process::id(),
            nanos,
            id
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn empty_directory_is_not_a_harness_home() {
        let root = temp_root();
        assert!(!is_harness_home(&root));
        fs::write(root.join(".credentials.yaml"), "DEEPSEEK_API_KEY: sk-test\n").unwrap();
        assert!(is_harness_home(&root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_missing_sessions_and_keys_without_overwriting() {
        let root = temp_root();
        let from = root.join("cli");
        let to = root.join("desktop");
        fs::create_dir_all(from.join("sessions").join("old")).unwrap();
        fs::write(from.join("sessions").join("old").join("log.jsonl"), "cli\n").unwrap();
        fs::write(from.join(".credentials.yaml"), "from: cli\n").unwrap();
        fs::write(from.join(".env"), "DEEPSEEK_API_KEY=cli\n").unwrap();
        fs::create_dir_all(to.join("sessions").join("old")).unwrap();
        fs::write(to.join("sessions").join("old").join("log.jsonl"), "desktop\n").unwrap();
        fs::write(to.join(".credentials.yaml"), "from: desktop\n").unwrap();

        let copied = import_missing(&from, &to).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            fs::read_to_string(to.join("sessions").join("old").join("log.jsonl")).unwrap(),
            "desktop\n"
        );
        assert_eq!(
            fs::read_to_string(to.join(".credentials.yaml")).unwrap(),
            "from: desktop\n"
        );
        assert_eq!(
            fs::read_to_string(to.join(".env")).unwrap(),
            "DEEPSEEK_API_KEY=cli\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_desktop_overlay_when_importing() {
        let root = temp_root();
        let from = root.join("cli");
        let to = root.join("desktop");
        fs::create_dir_all(from.join("desktop-overlay")).unwrap();
        fs::write(from.join("desktop-overlay").join("index.mjs"), "stolen\n").unwrap();
        fs::write(from.join(".env"), "DEEPSEEK_API_KEY=cli\n").unwrap();
        fs::create_dir_all(&to).unwrap();

        assert_eq!(import_missing(&from, &to).unwrap(), 1);
        assert!(!to.join("desktop-overlay").exists());
        assert!(to.join(".env").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_profile_node_modules_when_importing() {
        let root = temp_root();
        let from = root.join("cli");
        let to = root.join("desktop");
        fs::create_dir_all(from.join("profiles").join("node_modules").join("left-pad")).unwrap();
        fs::write(
            from.join("profiles").join("node_modules").join("left-pad").join("index.js"),
            "stolen\n",
        )
        .unwrap();
        fs::create_dir_all(from.join("sessions").join("keep")).unwrap();
        fs::write(from.join("sessions").join("keep").join("log.jsonl"), "cli\n").unwrap();
        fs::create_dir_all(&to).unwrap();

        let copied = import_missing(&from, &to).unwrap();
        assert!(copied >= 1);
        assert!(!to.join("profiles").join("node_modules").exists());
        assert!(to.join("sessions").join("keep").join("log.jsonl").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removes_leftover_profile_node_modules_from_the_selected_home() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        let cli = root.join("cli-home");
        fs::create_dir_all(cli.join("sessions").join("keep")).unwrap();
        fs::write(cli.join(".credentials.yaml"), "DEEPSEEK_API_KEY: cli\n").unwrap();
        fs::create_dir_all(cli.join("profiles").join("node_modules").join("junk")).unwrap();
        fs::write(
            cli.join("profiles").join("node_modules").join("junk").join("x.js"),
            "stolen\n",
        )
        .unwrap();
        fs::create_dir_all(cli.join("profiles").join("web")).unwrap();
        fs::write(cli.join("profiles").join("web").join("cordis.yml"), "name: web\n").unwrap();
        fs::create_dir_all(&isolated).unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(!cli.join("profiles").join("node_modules").exists());
        assert_eq!(
            fs::read_to_string(cli.join("profiles").join("web").join("cordis.yml")).unwrap(),
            "name: web\n"
        );
        assert!(cli.join("sessions").join("keep").is_dir());
        assert!(cli.join(".credentials.yaml").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn unlinks_profile_junctions_without_deleting_their_targets() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        let home = root.join("cli-home");
        let target = root.join("real-package");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), "target\n").unwrap();
        fs::create_dir_all(home.join("profiles").join("node_modules")).unwrap();
        fs::write(home.join(".credentials.yaml"), "k: v\n").unwrap();
        let link = home.join("profiles").join("node_modules").join("pkg");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &link.display().to_string(), &target.display().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        fs::create_dir_all(&isolated).unwrap();

        let _ = adopt_homes(&isolated, vec![home.clone()]).unwrap();
        assert!(!home.join("profiles").join("node_modules").exists());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "target\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keeps_intact_profile_node_modules() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        let cli = root.join("cli-home");
        let web = cli.join("profiles").join("web");
        fs::create_dir_all(web.join("node_modules").join("dsh-plugins-catalog")).unwrap();
        fs::write(
            web.join("package.json"),
            r#"{"name":"dsh-profile-web","dependencies":{"dsh-plugins-catalog":"github:x/y"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","dsh-plugins-catalog"]}}}"#,
        )
        .unwrap();
        fs::write(
            web.join("node_modules").join("dsh-plugins-catalog").join("package.json"),
            r#"{"name":"dsh-plugins-catalog","version":"0.2.0"}"#,
        )
        .unwrap();
        fs::create_dir_all(&isolated).unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(web.join("node_modules").join("dsh-plugins-catalog").join("package.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removes_profile_node_modules_when_a_dependency_is_missing() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        let cli = root.join("cli-home");
        let web = cli.join("profiles").join("web");
        fs::create_dir_all(web.join("node_modules").join("dsh-plugins-catalog")).unwrap();
        fs::write(
            web.join("package.json"),
            r#"{"dependencies":{"dsh-plugins-catalog":"github:x/y","gone-pkg":"1.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            web.join("node_modules").join("dsh-plugins-catalog").join("package.json"),
            r#"{"name":"dsh-plugins-catalog","version":"0.2.0"}"#,
        )
        .unwrap();
        fs::create_dir_all(&isolated).unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(!web.join("node_modules").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removes_profile_node_modules_without_a_profile_manifest() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        let cli = root.join("cli-home");
        let web = cli.join("profiles").join("web");
        fs::create_dir_all(web.join("node_modules").join("orphan")).unwrap();
        fs::write(web.join("cordis.yml"), "name: web\n").unwrap();
        fs::create_dir_all(&isolated).unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(!web.join("node_modules").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn isolated_home_stays_selected_when_no_cli_home_exists() {
        let root = temp_root();
        let isolated = root.join("dsh-home");
        fs::create_dir_all(&isolated).unwrap();
        let resolved = adopt_homes(&isolated, Vec::new()).unwrap();
        assert_eq!(resolved.path, isolated);
        assert_eq!(resolved.imported, 0);
        assert!(user_home_status(&resolved, &isolated).contains("未发现已有对话"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prefers_cli_home_and_imports_desktop_only_sessions() {
        let root = temp_root();
        let cli = root.join("cli-home");
        let isolated = root.join("dsh-home");
        fs::create_dir_all(cli.join("sessions").join("from-cli")).unwrap();
        fs::write(cli.join(".credentials.yaml"), "DEEPSEEK_API_KEY: cli\n").unwrap();
        fs::create_dir_all(isolated.join("sessions").join("from-desktop")).unwrap();
        fs::write(
            isolated.join("sessions").join("from-desktop").join("log.jsonl"),
            "desktop\n",
        )
        .unwrap();

        let resolved = adopt_homes(&isolated, vec![cli.clone(), isolated.clone()]).unwrap();
        assert_eq!(resolved.path, cli);
        assert!(resolved.imported >= 1);
        assert!(cli.join("sessions").join("from-desktop").is_dir());
        assert_eq!(
            fs::read_to_string(cli.join(".credentials.yaml")).unwrap(),
            "DEEPSEEK_API_KEY: cli\n"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
