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

/// Severity for a log line. Rendered as a fixed-width tag so the log is easy to
/// grep (`grep ERROR client.log`) and to scan by eye.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// Point the persistent logger at a file (called from app setup).
pub fn set_log_path(path: PathBuf) {
    let _ = LOG_FILE.set(path);
}

/// Path to the persistent diagnostics log, if configured.
pub fn log_path() -> Option<PathBuf> {
    LOG_FILE.get().cloned()
}

/// Read the last `max_lines` lines of the persistent diagnostics log.
/// Returns an empty string when the log is unset or unreadable.
pub fn log_tail(max_lines: usize) -> String {
    let Some(path) = LOG_FILE.get() else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Format a UNIX timestamp (seconds) as a human-readable UTC string
/// `YYYY-MM-DDThh:mm:ssZ`. Avoids pulling in `chrono` for one line.
fn fmt_utc(secs: u64) -> String {
    // Days since the UNIX epoch and the seconds within the current day.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// The current wall-clock time as an ISO-8601 UTC string. Used in report headers.
pub fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fmt_utc(secs)
}

/// Append a leveled, timestamped line to the persistent diagnostics log.
/// Best-effort: never panics and never blocks the caller on I/O errors.
pub fn log(level: Level, msg: &str) {
    if let Some(path) = LOG_FILE.get() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{}] {} {}", fmt_utc(secs), level.tag(), msg);
        }
    }
}

/// Back-compat INFO-level event log (existing call sites keep working).
pub fn log_event(msg: &str) {
    log(Level::Info, msg);
}

/// Convenience helpers for each level.
pub fn log_debug(msg: &str) {
    log(Level::Debug, msg);
}
pub fn log_warn(msg: &str) {
    log(Level::Warn, msg);
}
pub fn log_error(msg: &str) {
    log(Level::Error, msg);
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
    log_event("ensure_runtime: begin");
    let venv = venv_dir().ok_or_else(|| {
        log_error("ensure_runtime: runtime directory not initialised");
        "runtime directory not initialised".to_string()
    })?;
    log_event(&format!("ensure_runtime: venv path = {}", venv.display()));

    if venv_python().is_none() {
        let py = find_real_python().ok_or_else(|| {
            log_error("ensure_runtime: no real Python interpreter found on this system");
            "Python 3.11+ is required to create the runtime".to_string()
        })?;
        log_event(&format!("ensure_runtime: creating venv with interpreter {py}"));
        let _ = on_line.send(format!("Creating isolated runtime at {}", venv.display()));
        if let Some(parent) = venv.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut c = command(&py);
        // `--upgrade-deps` ensures pip/setuptools are present and current inside
        // the fresh venv, so the matrix-cli install below cannot fail on an
        // ancient bundled pip.
        c.arg("-m").arg("venv").arg("--upgrade-deps").arg(&venv);
        let code = stream(c, on_line).map_err(|e| e.to_string())?;
        if code != 0 || venv_python().is_none() {
            log_error(&format!(
                "ensure_runtime: venv creation failed (exit {code}, python present: {})",
                venv_python().is_some()
            ));
            return Err("failed to create the virtual environment".to_string());
        }
        log_event("ensure_runtime: venv created successfully");
    } else {
        log_event("ensure_runtime: reusing existing venv");
    }

    let vpy = venv_python()
        .ok_or_else(|| {
            log_error("ensure_runtime: managed Python missing after venv creation");
            "managed Python missing after venv creation".to_string()
        })?
        .to_string_lossy()
        .to_string();

    // Upgrade pip (best effort), then install/upgrade matrix-cli.
    log_event("ensure_runtime: upgrading pip");
    let mut c = command(&vpy);
    c.args(["-m", "pip", "install", "--upgrade", "pip"]);
    let _ = stream(c, on_line);

    log_event("ensure_runtime: installing/upgrading matrix-cli");
    let _ = on_line.send("Installing matrix-cli into the managed runtime…".into());
    let mut c = command(&vpy);
    c.args(["-m", "pip", "install", "--upgrade", "matrix-cli"]);
    let code = stream(c, on_line).map_err(|e| e.to_string())?;

    let ok = code == 0 && venv_matrix().is_some();
    if ok {
        log_event(&format!(
            "ensure_runtime: success — matrix at {}",
            venv_matrix().map(|p| p.display().to_string()).unwrap_or_default()
        ));
    } else {
        log_error(&format!(
            "ensure_runtime: matrix-cli install incomplete (pip exit {code}, matrix present: {})",
            venv_matrix().is_some()
        ));
    }
    Ok(ok)
}

/// A machine- and human-readable snapshot of the managed runtime, used by the
/// `runtime_diagnostics` command and logged at startup. This is the single
/// source of truth for "is the program ready to work?".
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    /// True when a real Python interpreter is available to build the venv.
    pub python_ok: bool,
    pub python_version: Option<String>,
    /// Absolute path to the managed venv directory (if the runtime dir is set).
    pub venv_path: Option<String>,
    /// True when the venv's own Python exists on disk.
    pub venv_python_ok: bool,
    /// True when the venv's `matrix` executable exists on disk.
    pub matrix_installed: bool,
    pub matrix_path: Option<String>,
    pub matrix_version: Option<String>,
    /// Overall readiness: a usable matrix CLI exists (venv or system PATH).
    pub ready: bool,
}

/// Gather a full snapshot of runtime readiness and write it to the log.
/// Call this at startup and from the Settings/Logs panel to confirm that the
/// Python `.venv` backend is installed and the program is ready to work.
pub fn runtime_diagnostics() -> RuntimeDiagnostics {
    let (python_ok, python_version) = python_status();
    let venv_path = venv_dir().map(|p| p.display().to_string());
    let venv_python_ok = venv_python().is_some();
    let matrix_path = venv_matrix().map(|p| p.display().to_string());
    let matrix_installed = matrix_path.is_some();
    let matrix_version = if cli_exists() { matrix_version() } else { None };
    let ready = cli_exists();

    let diag = RuntimeDiagnostics {
        python_ok,
        python_version,
        venv_path,
        venv_python_ok,
        matrix_installed,
        matrix_path,
        matrix_version,
        ready,
    };

    log(
        if diag.ready { Level::Info } else { Level::Warn },
        &format!(
            "runtime diagnostics: ready={} python_ok={} ({}) venv_python_ok={} matrix_installed={} matrix_version={} venv={}",
            diag.ready,
            diag.python_ok,
            diag.python_version.as_deref().unwrap_or("-"),
            diag.venv_python_ok,
            diag.matrix_installed,
            diag.matrix_version.as_deref().unwrap_or("-"),
            diag.venv_path.as_deref().unwrap_or("-"),
        ),
    );
    diag
}

/// Capture `<program> <args...>` and return (exit_code, combined stdout+stderr,
/// truncated to `max_chars`). Used by the diagnosis report to probe tools like
/// `matrix doctor` without streaming. Never panics; returns an Err string on
/// spawn failure so the report can show "could not run".
pub fn capture(program: &str, args: &[&str], max_chars: usize) -> Result<(i32, String), String> {
    let mut cmd = command(program);
    cmd.args(args);
    let out = cmd.output().map_err(|e| e.to_string())?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let text = text.trim().to_string();
    let text = if text.chars().count() > max_chars {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{cut}\n…(truncated)")
    } else {
        text
    };
    Ok((out.status.code().unwrap_or(-1), text))
}

/// Scan the persistent log and return the most recent lines tagged ERROR or
/// WARN (the leveled logger emits fixed-width `ERROR`/`WARN ` tags). Returns at
/// most `max` lines, newest last. Empty when nothing notable is logged.
pub fn recent_problems(max: usize) -> Vec<String> {
    let Some(path) = LOG_FILE.get() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut hits: Vec<String> = content
        .lines()
        .filter(|l| l.contains("] ERROR ") || l.contains("] WARN ") || l.contains("read error") || l.contains("write failed"))
        .map(|l| l.to_string())
        .collect();
    let start = hits.len().saturating_sub(max);
    hits.drain(0..start);
    hits
}

/// A redacted snapshot of environment values relevant to launching tools.
/// Home directories are kept (they are needed to interpret paths) but nothing
/// secret is included.
pub fn env_snapshot() -> Vec<(String, String)> {
    let keys = [
        "OS",
        "PROCESSOR_ARCHITECTURE",
        "SHELL",
        "COMSPEC",
        "SystemRoot",
        "VIRTUAL_ENV",
        "PYTHONHOME",
    ];
    let mut out = Vec::new();
    for k in keys {
        if let Ok(v) = std::env::var(k) {
            if !v.is_empty() {
                out.push((k.to_string(), v));
            }
        }
    }
    // PATH is long; record only how many entries and whether the venv is on it.
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let entries: Vec<&str> = path.split(sep).filter(|s| !s.is_empty()).collect();
        let venv_on_path = venv_bin_dir()
            .map(|b| {
                let b = b.to_string_lossy().to_lowercase();
                entries.iter().any(|e| e.to_lowercase() == b)
            })
            .unwrap_or(false);
        out.push(("PATH entries".to_string(), entries.len().to_string()));
        out.push(("venv on PATH".to_string(), venv_on_path.to_string()));
    }
    out
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
                log_debug(&format!("  out| {line}"));
                let _ = ch.send(line);
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let ch = on_line.clone();
        handles.push(thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log_warn(&format!("  err| {line}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_utc_is_iso_8601_shaped() {
        let s = now_utc();
        // YYYY-MM-DDThh:mm:ssZ
        assert_eq!(s.len(), 20, "got {s}");
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }

    #[test]
    fn capture_runs_a_real_program() {
        // Probe an interpreter that exists on every CI runner.
        let prog = if cfg!(windows) { "cmd" } else { "echo" };
        let args: &[&str] = if cfg!(windows) { &["/C", "echo", "hi"] } else { &["hi"] };
        let (code, out) = capture(prog, args, 100).expect("echo should run");
        assert_eq!(code, 0);
        assert!(out.contains("hi"), "got {out:?}");
    }

    #[test]
    fn capture_reports_missing_program() {
        assert!(capture("definitely-not-a-real-binary-xyz", &[], 100).is_err());
    }

    #[test]
    fn capture_truncates_long_output() {
        // Ask for a tiny budget and confirm the truncation marker appears.
        let prog = if cfg!(windows) { "cmd" } else { "printf" };
        let args: &[&str] = if cfg!(windows) {
            &["/C", "echo", "aaaaaaaaaaaaaaaaaaaa"]
        } else {
            &["aaaaaaaaaaaaaaaaaaaa"]
        };
        let (_code, out) = capture(prog, args, 5).expect("runs");
        assert!(out.contains("truncated"), "got {out:?}");
    }

    #[test]
    fn env_snapshot_records_path_metadata() {
        // PATH is set in every test environment, so the snapshot must include
        // the derived "PATH entries" and "venv on PATH" rows.
        let snap = env_snapshot();
        assert!(snap.iter().any(|(k, _)| k == "PATH entries"));
        assert!(snap.iter().any(|(k, _)| k == "venv on PATH"));
    }

    #[test]
    fn recent_problems_is_empty_without_log() {
        // With no log path configured in this unit-test binary, problem scan is
        // a clean empty vec rather than a panic.
        assert!(recent_problems(10).is_empty());
    }

    #[test]
    fn fmt_utc_matches_known_timestamps() {
        // Vectors generated with Python's datetime.utcfromtimestamp.
        assert_eq!(fmt_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(fmt_utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(fmt_utc(1_609_459_200), "2021-01-01T00:00:00Z");
        assert_eq!(fmt_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(fmt_utc(1_735_689_599), "2024-12-31T23:59:59Z");
        // Leap day (year 2000 is a leap year) exercises the civil-date math.
        assert_eq!(fmt_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn level_tags_are_fixed_width_and_distinct() {
        for l in [Level::Debug, Level::Info, Level::Warn, Level::Error] {
            assert_eq!(l.tag().len(), 5);
        }
        assert_eq!(Level::Error.tag().trim(), "ERROR");
        assert_eq!(Level::Info.tag().trim(), "INFO");
    }

    #[test]
    fn exe_name_is_platform_correct() {
        if cfg!(windows) {
            assert_eq!(exe_name("python"), "python.exe");
            assert_eq!(exe_name("matrix"), "matrix.exe");
        } else {
            assert_eq!(exe_name("python"), "python");
            assert_eq!(exe_name("matrix"), "matrix");
        }
    }

    #[test]
    fn venv_bin_dir_uses_platform_subdir() {
        // RUNTIME_DIR is a process-global OnceLock; set it once for this test
        // binary (first writer wins, fine for a read-only assertion).
        let base = std::env::temp_dir().join("mhc-test-runtime");
        set_runtime_dir(base);
        let bin = venv_bin_dir().expect("runtime dir is set");
        let s = bin.to_string_lossy().to_string();
        if cfg!(windows) {
            assert!(s.ends_with("Scripts"), "got {s}");
        } else {
            assert!(s.ends_with("bin"), "got {s}");
        }
        assert!(s.contains(".venv"), "got {s}");
    }

    #[test]
    fn diagnostics_are_internally_consistent() {
        // In the test sandbox no real venv exists, so matrix must not be
        // reported installed unless the venv binary genuinely exists on disk.
        let d = runtime_diagnostics();
        assert_eq!(d.matrix_installed, venv_matrix().is_some());
        if d.matrix_installed {
            assert!(d.matrix_path.is_some());
        }
    }

    #[test]
    fn split_args_edge_cases() {
        assert!(split_args("").is_empty());
        assert!(split_args("   ").is_empty());
        assert_eq!(split_args("one"), vec!["one"]);
        assert_eq!(split_args("  a   b  "), vec!["a", "b"]);
        assert_eq!(split_args("'single quoted'"), vec!["single quoted"]);
        assert_eq!(
            split_args("mix \"double q\" 'single q' bare"),
            vec!["mix", "double q", "single q", "bare"]
        );
        // An empty quoted string yields one empty argument.
        assert_eq!(split_args("a \"\" b"), vec!["a", "", "b"]);
    }
}
