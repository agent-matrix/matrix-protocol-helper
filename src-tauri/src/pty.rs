//! Real cross-platform terminal backed by a pseudo-terminal (PTY).
//!
//! Spawns the user's real shell (PowerShell/cmd on Windows, `$SHELL` on
//! Unix) attached to a ConPTY/Unix PTY via `portable-pty`, and bridges it to
//! an xterm.js terminal in the frontend: PTY output → `Channel<Vec<u8>>`,
//! keystrokes/resize → `pty_write` / `pty_resize`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::Channel;

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

static SESSIONS: OnceLock<Mutex<HashMap<u32, Session>>> = OnceLock::new();
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn sessions() -> &'static Mutex<HashMap<u32, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

/// The interactive shell to launch for the terminal.
fn default_shell() -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        if which::which("pwsh.exe").is_ok() {
            ("pwsh.exe".into(), vec!["-NoLogo".into()])
        } else if which::which("powershell.exe").is_ok() {
            ("powershell.exe".into(), vec!["-NoLogo".into()])
        } else {
            ("cmd.exe".into(), vec![])
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        // Interactive so the user's prompt/profile (and PATH) are loaded.
        (shell, vec!["-i".into()])
    }
}

/// Open a PTY + shell. Streams raw output bytes to `on_data`; returns a session id.
pub fn open(on_data: Channel<Vec<u8>>, cols: u16, rows: u16) -> Result<u32, String> {
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

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);

    // Pump PTY output to the frontend.
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if on_data.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
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
    if let Some(s) = map.get_mut(&id) {
        s.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        s.writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    let map = sessions().lock().unwrap();
    if let Some(s) = map.get(&id) {
        s.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn close(id: u32) {
    if let Some(mut s) = sessions().lock().unwrap().remove(&id) {
        let _ = s.child.kill();
    }
}

/* ---------- Tauri commands ---------- */

#[tauri::command]
pub fn pty_open(on_data: Channel<Vec<u8>>, cols: u16, rows: u16) -> Result<u32, String> {
    open(on_data, cols, rows)
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
