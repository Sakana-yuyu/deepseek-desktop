use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use super::app_data_root;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Open `%APPDATA%/DeepSeek Harness/boot.log` for append-only boot diagnostics.
pub fn init() -> Result<(), String> {
    let root = app_data_root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join("boot.log");
    LOG_PATH
        .set(path.clone())
        .map_err(|_| "boot log already initialized".to_string())?;
    info("=== boot session started ===");
    Ok(())
}

/// Append one timestamped line to the boot log (best-effort).
pub fn info(message: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let line = format!("{} {message}\n", timestamp());
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

/// Append one timestamped error line to the boot log (best-effort).
pub fn error(message: &str) {
    info(&format!("ERROR {message}"));
}

fn timestamp() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}
