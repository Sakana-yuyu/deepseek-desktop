//! Implant the desktop overlay plugin into `$DSH_HOME` without editing packages.

use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::provision::RuntimePaths;

/// Absolute `--patch` file the Host should load.
pub struct OverlayPatch {
    pub patch_file: PathBuf,
}

/// Copy the overlay plugin into the selected home and write a `--patch` list.
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
    fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("无法创建 {}: {e}", dest_dir.display()))?;
    let plugin_dest = dest_dir.join("index.mjs");
    fs::copy(&plugin_src, &plugin_dest)
        .map_err(|e| format!("无法复制 {} -> {}: {e}", plugin_src.display(), plugin_dest.display()))?;

    let plugin_path = normalize_plugin_path(&plugin_dest)?;
    let patch_file = dest_dir.join("cordis.yml");
    let yaml = format!(
        "- insert:\n    - id: dsh-desktop-notify\n      name: '{plugin_path}'\n"
    );
    fs::write(&patch_file, yaml)
        .map_err(|e| format!("无法写入 {}: {e}", patch_file.display()))?;

    let _ = notify_url;
    Ok(OverlayPatch { patch_file })
}

fn normalize_plugin_path(path: &Path) -> Result<String, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("无法解析 {}: {e}", path.display()))?;
    url::Url::from_file_path(&canonical)
        .map(|file_url| file_url.as_str().to_string())
        .map_err(|()| {
            format!(
                "plugin path is not a usable file URL: {}",
                canonical.display()
            )
        })
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
    fn writes_a_file_url_patch_row_that_node_esm_can_import() {
        let root = std::env::temp_dir().join(format!(
            "dsh desktop overlay {}",
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
        let plugin = dsh_home.join("desktop-overlay").join("index.mjs");
        let plugin_url = normalize_plugin_path(&plugin).unwrap();
        assert!(plugin.is_file());
        assert!(plugin_url.starts_with("file://"), "{plugin_url}");
        assert!(plugin_url.contains("%20"), "{plugin_url}");
        assert!(!plugin_url.contains('\\'), "{plugin_url}");
        assert!(yaml.contains("id: dsh-desktop-notify"));
        assert!(yaml.contains(&format!("name: '{plugin_url}'")), "{yaml}");
        let _ = fs::remove_dir_all(&root);
    }
}
