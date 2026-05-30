//! Low-level helpers around the `matrix` CLI and local environment.
//! Output is streamed to the frontend via a Tauri IPC `Channel<String>`.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use tauri::ipc::Channel;
use which::which;

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

/// Checks if the `matrix` CLI command exists in the system's PATH.
pub fn cli_exists() -> bool {
    which("matrix").is_ok()
}

/// Returns the `matrix --version` string (first line) when available.
/// Reads stdout *and* stderr, since some CLIs print version info to stderr.
pub fn matrix_version() -> Option<String> {
    let out = command("matrix").arg("--version").output().ok()?;
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

    let mut child = cmd.spawn()?;
    let mut handles: Vec<thread::JoinHandle<()>> = vec![];

    if let Some(stdout) = child.stdout.take() {
        let ch = on_line.clone();
        handles.push(thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = ch.send(line);
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let ch = on_line.clone();
        handles.push(thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = ch.send(line);
            }
        }));
    }

    let status = child.wait()?;
    for h in handles {
        let _ = h.join();
    }
    Ok(status.code().unwrap_or(-1))
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
