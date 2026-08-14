//! Implant the desktop overlay plugin into `$DSH_HOME` without editing packages.

use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::provision::RuntimePaths;

/// Absolute `--patch` file the Host should load.
pub struct OverlayPatch {
    pub patch_file: PathBuf,
}

/// Copy the overlay plugin into the isolated home and write a `--patch` list.
pub fn install_overlay(
    paths: &RuntimePaths,
    overlay_src: &Path,
    notify_url: &str,
) -> Result<OverlayPatch, String> {
    let plugin_src = overlay_src.join("index.mjs");
    if !plugin_src.is_file() {
        return Err(format!(
            "desktop overlay plugin missing: {}",
            plugin_src.display()
        ));
    }

    let dest_dir = paths.dsh_home.join("desktop-overlay");
    fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let plugin_dest = dest_dir.join("index.mjs");
    fs::copy(&plugin_src, &plugin_dest).map_err(|e| e.to_string())?;

    let plugin_path = normalize_plugin_path(&plugin_dest)?;
    let patch_file = dest_dir.join("cordis.yml");
    let yaml = format!(
        "- insert:\n    - id: dsh-desktop-notify\n      name: '{plugin_path}'\n"
    );
    fs::write(&patch_file, yaml).map_err(|e| e.to_string())?;

    let _ = notify_url;
    Ok(OverlayPatch { patch_file })
}

fn normalize_plugin_path(path: &Path) -> Result<String, String> {
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    let mut text = canonical.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = text.strip_prefix("//?/") {
        text = stripped.to_string();
    }
    Ok(text)
}

/// Resolve the overlay source shipped beside the desktop shell.
pub fn resolve_overlay_source(resource_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = resource_dir {
        let bundled = dir.join("desktop-overlay");
        if bundled.join("index.mjs").is_file() {
            return bundled;
        }
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("overlay")
        .join("desktop-notify")
}

#[cfg(test)]
mod tests {
    use super::{install_overlay, normalize_plugin_path};
    use crate::runtime::provision::RuntimePaths;
    use std::fs;

    #[test]
    fn writes_an_absolute_patch_row_without_backslashes() {
        let root = std::env::temp_dir().join(format!(
            "dsh-desktop-overlay-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src").join("index.mjs"), "export function apply() {}\n").unwrap();

        let dsh_home = root.join("home");
        let paths = RuntimePaths {
            node_binary: root.join("node"),
            pnpm_binary: root.join("pnpm"),
            cli_entry: root.join("bin.js"),
            harness_root: root.clone(),
            runtime_root: root.join("runtime"),
            dsh_home: dsh_home.clone(),
        };

        let overlay = install_overlay(&paths, &root.join("src"), "http://127.0.0.1:9/notify").unwrap();
        let yaml = fs::read_to_string(&overlay.patch_file).unwrap();
        assert!(yaml.contains("id: dsh-desktop-notify"));
        assert!(yaml.contains("name: '"));
        assert!(!yaml.contains('\\'));
        let plugin = dsh_home.join("desktop-overlay").join("index.mjs");
        assert!(plugin.is_file());
        assert!(!normalize_plugin_path(&plugin).unwrap().contains('\\'));
        let _ = fs::remove_dir_all(&root);
    }
}
