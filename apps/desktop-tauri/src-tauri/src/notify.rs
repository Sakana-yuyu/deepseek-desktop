//! Localhost notify endpoint, system toast, and completion sound.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;

use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::runtime::boot_log;

#[derive(Clone)]
pub struct NotifyHandle {
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct NotifyPayload {
    title: Option<String>,
    body: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

/// Bind `127.0.0.1:0` and serve POST /notify for the overlay plugin.
pub fn start(app: AppHandle) -> Result<NotifyHandle, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let url = format!("http://127.0.0.1:{port}/notify");
    boot_log::info(&format!("desktop notify listening {url}"));

    let sound = resolve_sound_path(&app);
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(stream) = incoming else {
                continue;
            };
            let app = app.clone();
            let sound = sound.clone();
            thread::spawn(move || handle_client(app, stream, sound.as_deref()));
        }
    });

    Ok(NotifyHandle { url })
}

fn resolve_sound_path(app: &AppHandle) -> Option<PathBuf> {
    let resource = app.path().resource_dir().ok()?.join("complete.wav");
    if resource.is_file() {
        return Some(resource);
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sounds")
        .join("complete.wav");
    dev.is_file().then_some(dev)
}

fn handle_client(app: AppHandle, mut stream: TcpStream, sound: Option<&std::path::Path>) {
    let peer = stream.peer_addr().ok();
    if !peer.is_some_and(|addr| addr.ip().is_loopback()) {
        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    if !request.starts_with("POST /notify") {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
    let payload: NotifyPayload = serde_json::from_str(body.trim_end_matches('\0')).unwrap_or(
        NotifyPayload {
            title: None,
            body: None,
            session_id: None,
        },
    );
    let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");

    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return;
    }

    let title = payload.title.unwrap_or_else(|| "任务完成".into());
    let body = payload
        .body
        .or(payload.session_id.map(|id| format!("会话 {id} 已完成")))
        .unwrap_or_else(|| "DeepSeek Harness 已完成本轮任务".into());

    if let Err(error) = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        boot_log::error(&format!("desktop notify toast failed: {error}"));
    }

    if let Some(path) = sound {
        play_wav(path);
    }
}

fn play_wav(path: &std::path::Path) {
    #[cfg(windows)]
    {
        play_wav_windows(path);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("afplay").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        for bin in ["paplay", "aplay", "ffplay"] {
            let mut cmd = std::process::Command::new(bin);
            if bin == "ffplay" {
                cmd.args(["-nodisp", "-autoexit", "-loglevel", "quiet"]);
            }
            if cmd.arg(path).spawn().is_ok() {
                break;
            }
        }
    }
}

#[cfg(windows)]
fn play_wav_windows(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(psz_sound: *const u16, hmod: *mut core::ffi::c_void, fdw_sound: u32) -> i32;
    }

    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_FILENAME: u32 = 0x0002_0000;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_ASYNC | SND_NODEFAULT | SND_FILENAME,
        );
    }
}

/// Show a shell-owned toast (tray update status, etc.).
pub fn toast(app: &AppHandle, title: &str, body: &str) {
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        boot_log::error(&format!("desktop toast failed: {error}"));
    }
}
