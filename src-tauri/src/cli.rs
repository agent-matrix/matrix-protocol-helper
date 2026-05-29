//! Low-level helpers around the `matrix` CLI and local environment.
//! Output is streamed to the frontend via a Tauri IPC `Channel<String>`.

use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use which::which;

/// Checks if the `matrix` CLI command exists in the system's PATH.
pub fn cli_exists() -> bool {
    which("matrix").is_ok()
}

/// Returns the `matrix --version` string (first line) when available.
pub fn matrix_version() -> Option<String> {
    let out = Command::new("matrix").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// Detects a Python interpreter and its version (`python3` preferred).
pub fn python_status() -> (bool, Option<String>) {
    for bin in ["python3", "python"] {
        if which(bin).is_err() {
            continue;
        }
        if let Ok(out) = Command::new(bin).arg("--version").output() {
            // Older Python prints the version to stderr.
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let raw = if stdout.trim().is_empty() { stderr } else { stdout };
            let ver = raw.trim().replace("Python", "").trim().to_string();
            return (true, if ver.is_empty() { None } else { Some(ver) });
        }
        return (true, None);
    }
    (false, None)
}

/// Spawns a command (no shell) and streams stdout+stderr line-by-line into
/// `on_line`, returning the process exit code.
pub fn stream(mut cmd: Command, on_line: &Channel<String>) -> std::io::Result<i32> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

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
