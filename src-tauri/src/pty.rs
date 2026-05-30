//! Real cross-platform terminal backed by a pseudo-terminal (PTY).
//!
//! Spawns the user's real shell (PowerShell/cmd on Windows, `$SHELL` on
//! Unix) attached to a ConPTY/Unix PTY via `portable-pty`, and bridges it to
//! an xterm.js terminal in the frontend: PTY output → Tauri events ("pty-data-{id}"),
//! keystrokes/resize → `pty_write` / `pty_resize`.
//!
//! Transport note: PTY bytes are base64-encoded and delivered via
//! `AppHandle::emit()` (Tauri's thread-safe event bus) rather than
//! `Channel<T>::send()`. The Channel transport proved unreliable on Windows
//! WebView2 and some Linux WebKitGTK builds when called from background
//! threads — messages were enqueued in Rust but the JS onmessage callback
//! was never fired. `emit()` routes through Tauri's main event loop and
//! is documented as safe to call from any thread.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

static SESSIONS: OnceLock<Mutex<HashMap<u32, Session>>> = OnceLock::new();
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Lifetime PTY metrics, surfaced in the diagnosis report so terminal issues
/// (e.g. a shell that opens but never emits, or many short-lived sessions from
/// a remount loop) are visible without reproducing the bug live.
static TOTAL_OPENED: AtomicU32 = AtomicU32::new(0);
static TOTAL_CLOSED: AtomicU32 = AtomicU32::new(0);
static TOTAL_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Payload for each "pty-data-{id}" event emitted to the frontend.
#[derive(Clone, Serialize)]
pub struct PtyDataEvent {
    /// Base64-encoded PTY output chunk.
    pub data: String,
}

/// A snapshot of PTY activity for diagnostics.
pub struct PtyStats {
    pub opened: u32,
    pub closed: u32,
    pub currently_open: usize,
    pub bytes_streamed: u64,
}

/// Read the current PTY metrics (lock-free counters + live session count).
pub fn stats() -> PtyStats {
    let opened = TOTAL_OPENED.load(Ordering::Relaxed);
    let closed = TOTAL_CLOSED.load(Ordering::Relaxed);
    let bytes_streamed = TOTAL_BYTES.load(Ordering::Relaxed);
    let currently_open = sessions().lock().map(|m| m.len()).unwrap_or(0);
    PtyStats {
        opened,
        closed,
        currently_open,
        bytes_streamed,
    }
}

fn sessions() -> &'static Mutex<HashMap<u32, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

/// Return the first executable that exists. On Windows GUI apps can start with
/// a reduced PATH, so also check well-known absolute locations instead of
/// relying only on `which`.
#[cfg(target_os = "windows")]
fn first_existing(candidates: &[String]) -> Option<String> {
    candidates
        .iter()
        .find(|p| std::path::Path::new(p.as_str()).exists() || which::which(p.as_str()).is_ok())
        .cloned()
}

/// The interactive shell to launch for the terminal.
fn default_shell() -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let candidates = vec![
            "pwsh.exe".to_string(),
            format!(r"{}\System32\WindowsPowerShell\v1.0\powershell.exe", system_root),
            "powershell.exe".to_string(),
            std::env::var("COMSPEC").unwrap_or_else(|_| format!(r"{}\System32\cmd.exe", system_root)),
            "cmd.exe".to_string(),
        ];

        let shell = first_existing(&candidates).unwrap_or_else(|| "cmd.exe".into());
        let lower = shell.to_ascii_lowercase();
        if lower.ends_with("pwsh.exe") || lower.ends_with("powershell.exe") {
            // -NoExit keeps the shell interactive even if profile/startup code
            // writes to stderr or stdin is initially quiet.
            (shell, vec!["-NoLogo".into(), "-NoExit".into()])
        } else {
            (shell, vec![])
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        // Interactive so the user's prompt/profile (and PATH) are loaded.
        (shell, vec!["-i".into()])
    }
}

/// Standard base64 (RFC 4648) encoder. PTY output is base64-encoded before
/// being emitted as a string event so that arbitrary binary bytes (escape
/// sequences, high bytes) survive the JSON/IPC boundary intact.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Open a PTY + shell. Streams base64-encoded output via Tauri events
/// ("pty-data-{id}"); returns the session id.
pub fn open(app: AppHandle, cols: u16, rows: u16) -> Result<u32, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let (shell, args) = default_shell();
    crate::cli::log_event(&format!("opening PTY shell: {} {:?}", shell, args));
    let mut cmd = CommandBuilder::new(&shell);
    for a in args {
        cmd.arg(a);
    }
    if let Some(home) = home_dir() {
        cmd.cwd(home);
    }
    // Ensure child CLIs emit UTF-8 (matches the rest of the app).
    cmd.env("PYTHONUTF8", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("TERM", "xterm-256color");
    // Put the app-managed venv first on PATH so `matrix` in the terminal
    // resolves to the runtime the client installed.
    if let Some(bin) = crate::cli::venv_bin_dir() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let cur = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}{}{}", bin.display(), sep, cur));
    }

    // Clone the master's reader/writer BEFORE spawning, then spawn the child.
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;

    // Drop the slave handle now that the child owns its own copy. This is
    // REQUIRED on Windows: while we still hold the slave, ConPTY keeps the
    // write end open, so the master's read blocks forever and the shell prompt
    // never reaches the frontend ("PTY opened, but no prompt yet"). On Unix it
    // is harmless but equally correct.
    drop(pair.slave);

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    TOTAL_OPENED.fetch_add(1, Ordering::Relaxed);
    crate::cli::log_event(&format!("PTY session {id} spawned; streaming output via event pty-data-{id}"));

    // Pump PTY output to the frontend via Tauri events. AppHandle::emit() is
    // safe to call from any thread and routes through Tauri's main event loop,
    // avoiding the WebView2/WebKitGTK reliability issues of Channel::send().
    let app_thread = app.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut total: usize = 0;
        let event_name = format!("pty-data-{id}");
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    crate::cli::log_event(&format!("PTY session {id} reader EOF ({total} bytes total)"));
                    // Signal the frontend that this session ended.
                    let _ = app_thread.emit(&format!("pty-exit-{id}"), ());
                    break;
                }
                Ok(n) => {
                    total += n;
                    TOTAL_BYTES.fetch_add(n as u64, Ordering::Relaxed);
                    let encoded = base64_encode(&buf[..n]);
                    if app_thread.emit(&event_name, PtyDataEvent { data: encoded }).is_err() {
                        crate::cli::log_warn(&format!("PTY session {id} emit failed; closing reader"));
                        break;
                    }
                }
                Err(e) => {
                    crate::cli::log_warn(&format!("PTY session {id} read error: {e}"));
                    break;
                }
            }
        }
    });

    sessions().lock().unwrap().insert(
        id,
        Session {
            master: pair.master,
            writer,
            child,
        },
    );
    Ok(id)
}

pub fn write(id: u32, data: String) -> Result<(), String> {
    let mut map = sessions().lock().unwrap();
    let Some(s) = map.get_mut(&id) else {
        return Err(format!("terminal session {id} is not open"));
    };
    s.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    s.writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    let map = sessions().lock().unwrap();
    let Some(s) = map.get(&id) else {
        return Err(format!("terminal session {id} is not open"));
    };
    s.master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn close(id: u32) {
    if let Some(mut s) = sessions().lock().unwrap().remove(&id) {
        TOTAL_CLOSED.fetch_add(1, Ordering::Relaxed);
        crate::cli::log_event(&format!("closing PTY session {id}"));
        let _ = s.child.kill();
    }
}

/* ---------- Tauri commands ---------- */

/// Open a PTY session. The session streams output via Tauri events named
/// "pty-data-{id}" (base64 payload) and "pty-exit-{id}" (shell closed).
/// Use `listen()` in the frontend to subscribe before or immediately after
/// this call returns.
#[tauri::command]
pub fn pty_open(app: AppHandle, cols: u16, rows: u16) -> Result<u32, String> {
    open(app, cols, rows)
}

#[tauri::command]
pub fn pty_write(id: u32, data: String) -> Result<(), String> {
    write(id, data)
}

#[tauri::command]
pub fn pty_resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    resize(id, cols, rows)
}

#[tauri::command]
pub fn pty_close(id: u32) {
    close(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 §10 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_high_bytes() {
        // Non-ASCII / control bytes (what a real PTY emits) must round-trip
        // through the table without panicking or producing invalid chars.
        let out = base64_encode(&[0x00, 0xff, 0x1b, 0x5b]);
        assert_eq!(out, "AP8bWw==");
        assert!(out.chars().all(|c| c.is_ascii()));
    }

    #[test]
    fn default_shell_is_usable() {
        let (shell, args) = default_shell();
        assert!(!shell.is_empty(), "a shell program must always be chosen");

        #[cfg(not(target_os = "windows"))]
        {
            // Unix shells launch interactively so the user's profile/PATH loads.
            assert!(args.iter().any(|a| a == "-i"));
        }
        #[cfg(target_os = "windows")]
        {
            // PowerShell variants must stay interactive (-NoExit); cmd takes none.
            let lower = shell.to_ascii_lowercase();
            if lower.ends_with("powershell.exe") || lower.ends_with("pwsh.exe") {
                assert!(args.iter().any(|a| a == "-NoExit"));
                assert!(args.iter().any(|a| a == "-NoLogo"));
            }
        }
    }

    #[test]
    fn write_to_missing_session_errors() {
        // No session id 0 is ever handed out (ids start at 1), so this must be
        // a clean Err rather than a silent success.
        let err = write(0, "echo hi\n".into()).unwrap_err();
        assert!(err.contains("not open"), "got: {err}");
    }

    #[test]
    fn resize_missing_session_errors() {
        let err = resize(0, 80, 24).unwrap_err();
        assert!(err.contains("not open"), "got: {err}");
    }

    #[test]
    fn close_missing_session_is_noop() {
        // Closing an unknown id must not panic.
        close(123_456);
    }

    #[test]
    fn stats_are_internally_consistent() {
        // With no terminals spawned in this unit-test binary, the live count is
        // zero and closed never exceeds opened.
        let s = stats();
        assert_eq!(s.currently_open, 0);
        assert!(s.closed <= s.opened);
    }
}
