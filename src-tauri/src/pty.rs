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
        crate::cli::log_event(&format!("closing PTY session {id}"));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
