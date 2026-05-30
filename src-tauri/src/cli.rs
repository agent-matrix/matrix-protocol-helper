//! Low-level helpers around the `matrix` CLI and local environment.
//! Output is streamed to the frontend via a Tauri IPC `Channel<String>`.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use tauri::ipc::Channel;
use which::which;

/// Path to the persistent diagnostics log (`client.log`), set once at startup.
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Point the persistent logger at a file (called from app setup).
pub fn set_log_path(path: PathBuf) {
    let _ = LOG_FILE.set(path);
}

/// Append a timestamped line to the persistent diagnostics log (best-effort).
pub fn log_event(msg: &str) {
    if let Some(path) = LOG_FILE.get() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{secs}] {msg}");
        }
    }
}

/// Creates a command configured for a GUI app. On Windows it suppresses the
/// extra console window that otherwise flashes/opens when a GUI-subsystem
/// Tauri app launches a console child process (matrix, python, pipx, …).
///
/// It also forces UTF-8, unbuffered I/O on child processes. Without this, a
/// Python CLI writing to a *pipe* on Windows encodes with the legacy ANSI code
/// page (cp1252); Unicode in `matrix --help` (em-dash, box-drawing chars) then
/// raises UnicodeEncodeError on flush, which makes Python exit with code 120
/// and swallows the output. `PYTHONUTF8`/`PYTHONIOENCODING` fix the encoding and
/// `PYTHONUNBUFFERED` makes output stream live into the embedded terminal.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    suppress_console_window(&mut cmd);
    cmd.env("PYTHONUTF8", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd
}

/// Applies the Windows no-console flag to an existing Command (no-op elsewhere).
pub fn suppress_console_window(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

/// Root for the app-managed runtime (`<app_data_dir>/runtime`), set at startup.
static RUNTIME_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Point the managed runtime at a directory (called from app setup).
pub fn set_runtime_dir(dir: PathBuf) {
    let _ = RUNTIME_DIR.set(dir);
}

fn venv_dir() -> Option<PathBuf> {
    RUNTIME_DIR.get().map(|d| d.join(".venv"))
}

/// The venv's `Scripts` (Windows) / `bin` (Unix) directory.
pub fn venv_bin_dir() -> Option<PathBuf> {
    venv_dir().map(|v| {
        if cfg!(windows) {
            v.join("Scripts")
        } else {
            v.join("bin")
        }
    })
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Path to the managed venv's Python, if the venv exists.
pub fn venv_python() -> Option<PathBuf> {
    venv_bin_dir()
        .map(|b| b.join(exe_name("python")))
        .filter(|p| p.exists())
}

/// Path to the managed venv's `matrix`, if installed.
pub fn venv_matrix() -> Option<PathBuf> {
    venv_bin_dir()
        .map(|b| b.join(exe_name("matrix")))
        .filter(|p| p.exists())
}

/// The `matrix` program to invoke: the managed venv's copy if present,
/// otherwise a system `matrix` on PATH (fallback).
pub fn matrix_program() -> String {
    venv_matrix()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "matrix".to_string())
}

/// Whether a usable Matrix CLI exists (managed venv or system PATH).
pub fn cli_exists() -> bool {
    venv_matrix().is_some() || which("matrix").is_ok()
}

/// Create the managed venv (if needed) and install/upgrade matrix-cli into it.
/// This is isolated from system Python, so it avoids PATH, Microsoft Store
/// alias, and system-`Scripts` issues entirely. Returns true on success.
pub fn ensure_runtime(on_line: &Channel<String>) -> Result<bool, String> {
    let venv = venv_dir().ok_or_else(|| "runtime directory not initialised".to_string())?;

    if venv_python().is_none() {
        let py = find_real_python()
            .ok_or_else(|| "Python 3.11+ is required to create the runtime".to_string())?;
        let _ = on_line.send(format!("Creating isolated runtime at {}", venv.display()));
        if let Some(parent) = venv.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut c = command(&py);
        c.arg("-m").arg("venv").arg(&venv);
        let code = stream(c, on_line).map_err(|e| e.to_string())?;
        if code != 0 || venv_python().is_none() {
            return Err("failed to create the virtual environment".to_string());
        }
    }

    let vpy = venv_python()
        .ok_or_else(|| "managed Python missing after venv creation".to_string())?
        .to_string_lossy()
        .to_string();

    // Upgrade pip (best effort), then install/upgrade matrix-cli.
    let mut c = command(&vpy);
    c.args(["-m", "pip", "install", "--upgrade", "pip"]);
    let _ = stream(c, on_line);

    let _ = on_line.send("Installing matrix-cli into the managed runtime…".into());
    let mut c = command(&vpy);
    c.args(["-m", "pip", "install", "--upgrade", "matrix-cli"]);
    let code = stream(c, on_line).map_err(|e| e.to_string())?;

    Ok(code == 0 && venv_matrix().is_some())
}

/// Delete the managed venv (for a clean repair).
pub fn remove_runtime() -> std::io::Result<()> {
    if let Some(venv) = venv_dir() {
        if venv.exists() {
            std::fs::remove_dir_all(venv)?;
        }
    }
    Ok(())
}

/// Returns the `matrix --version` string (first line) when available.
/// Reads stdout *and* stderr, since some CLIs print version info to stderr.
pub fn matrix_version() -> Option<String> {
    let out = command(matrix_program()).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = if stdout.trim().is_empty() { stderr } else { stdout };
    let line = text.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// True if `p` is the Windows "App Execution Alias" stub
/// (`…\Microsoft\WindowsApps\python.exe`) that only prints a Store message.
fn is_store_alias(p: &Path) -> bool {
    p.to_string_lossy().to_lowercase().contains("windowsapps")
}

/// Run `<program> --version`; return the trimmed version text only if the
/// process succeeds AND the output actually looks like Python (rejects the
/// Microsoft Store alias, which exits non-zero and prints a "not found" notice).
fn python_version_of(program: &str) -> Option<String> {
    let out = command(program).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let raw = if stdout.trim().is_empty() { stderr } else { stdout };
    let raw = raw.trim();
    if !raw.starts_with("Python ") {
        return None;
    }
    Some(raw.replacen("Python", "", 1).trim().to_string())
}

/// Find a **real** Python interpreter, skipping the Windows Store alias stub.
/// Prefers the `py` launcher (never a stub), then `python3`/`python`. Because
/// the bare `python` name often resolves to the stub *before* a real install
/// on PATH, we scan every match (`which_all`) and pick the first that works.
/// Returns the program/path to invoke.
pub fn find_real_python() -> Option<String> {
    // `py` (Windows launcher) and Unix `python3` rarely collide with the stub.
    for name in ["py", "python3", "python"] {
        if let Ok(paths) = which::which_all(name) {
            for p in paths {
                if is_store_alias(&p) {
                    continue;
                }
                let prog = p.to_string_lossy().to_string();
                if python_version_of(&prog).is_some() {
                    return Some(prog);
                }
            }
        }
    }
    None
}

/// Detects a real Python interpreter and its version.
pub fn python_status() -> (bool, Option<String>) {
    match find_real_python() {
        Some(prog) => (true, python_version_of(&prog)),
        None => (false, None),
    }
}

/// Spawns a command (no shell) and streams stdout+stderr line-by-line into
/// `on_line`, returning the process exit code.
pub fn stream(mut cmd: Command, on_line: &Channel<String>) -> std::io::Result<i32> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Belt-and-suspenders: ensure no console window even if the caller built
    // the Command with std::process::Command::new directly.
    suppress_console_window(&mut cmd);

    // Record the exact command in the persistent log for debugging.
    let prog = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    log_event(&format!("$ {} {}", prog, args.join(" ")));

    let mut child = cmd.spawn()?;
    let mut handles: Vec<thread::JoinHandle<()>> = vec![];

    if let Some(stdout) = child.stdout.take() {
        let ch = on_line.clone();
        handles.push(thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                log_event(&format!("  out| {line}"));
                let _ = ch.send(line);
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let ch = on_line.clone();
        handles.push(thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log_event(&format!("  err| {line}"));
                let _ = ch.send(line);
            }
        }));
    }

    let status = child.wait()?;
    for h in handles {
        let _ = h.join();
    }
    let code = status.code().unwrap_or(-1);
    log_event(&format!("[exit {code}] {prog}"));
    Ok(code)
}

/// Measures TCP connect latency (ms) to the hub URL's host:port.
pub fn ping_hub(url: &str) -> Result<u32, String> {
    let parsed = url::Url::parse(url).map_err(|_| "invalid hub URL".to_string())?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "no host in URL".to_string())?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let mut addrs = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("dns: {e}"))?;
    let addr = addrs
        .next()
        .ok_or_else(|| "could not resolve host".to_string())?;
    let start = Instant::now();
    TcpStream::connect_timeout(&addr, Duration::from_secs(4)).map_err(|e| format!("connect: {e}"))?;
    Ok(start.elapsed().as_millis() as u32)
}

/// Minimal, shell-free argument splitter that understands single/double quotes.
pub fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in line.trim().chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                has = true;
            }
            None if c.is_whitespace() => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}
