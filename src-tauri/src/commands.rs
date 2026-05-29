//! Tauri commands invoked by the MatrixHub Client frontend.

use std::process::Command;

use serde::Serialize;
use tauri::ipc::Channel;
use which::which;

use crate::cli;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    cli: bool,
    cli_version: Option<String>,
    python: bool,
    python_version: Option<String>,
}

/// Detect the local environment: Matrix CLI + Python.
#[tauri::command]
pub async fn cli_status() -> CliStatus {
    tauri::async_runtime::spawn_blocking(|| {
        let cli = cli::cli_exists();
        let cli_version = if cli { cli::matrix_version() } else { None };
        let (python, python_version) = cli::python_status();
        CliStatus {
            cli,
            cli_version,
            python,
            python_version,
        }
    })
    .await
    .unwrap_or(CliStatus {
        cli: false,
        cli_version: None,
        python: false,
        python_version: None,
    })
}

/// Measure hub connectivity, returning round-trip milliseconds.
#[tauri::command]
pub async fn test_hub(url: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || cli::ping_hub(&url))
        .await
        .map_err(|e| e.to_string())?
}

/// Find an available Python interpreter (`python3`/`python`/`py`).
fn find_python() -> Option<&'static str> {
    ["python3", "python", "py"].into_iter().find(|b| which(b).is_ok())
}

/// Whether `<py> -m <module> --version` succeeds (module importable).
fn has_module(py: &str, module: &str) -> bool {
    Command::new(py)
        .args(["-m", module, "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build the best-available CLI installer command.
///
/// Order is chosen for robustness — especially on Windows, where a plain
/// `pip install matrix-cli` into the system Python fails trying to replace
/// `dotenv.exe` (WinError 2):
///   1. `python -m pipx install …` — isolated, and works even when the freshly
///      installed `pipx` isn't on PATH yet.
///   2. bare `pipx install …` (pipx on PATH).
///   3. `python -m pip install --user …` — user site, avoids protected system
///      `Scripts`.
fn cli_installer() -> Option<Command> {
    let py = find_python();

    if let Some(p) = py {
        if has_module(p, "pipx") {
            let mut c = Command::new(p);
            c.args(["-m", "pipx", "install", "matrix-cli"]);
            return Some(c);
        }
    }
    if which("pipx").is_ok() {
        let mut c = Command::new("pipx");
        c.args(["install", "matrix-cli"]);
        return Some(c);
    }
    if let Some(p) = py {
        let mut c = Command::new(p);
        c.args(["-m", "pip", "install", "--user", "matrix-cli"]);
        return Some(c);
    }
    None
}

/// Install the Matrix CLI; streams output, resolves true on success.
#[tauri::command]
pub async fn install_cli(on_line: Channel<String>) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || match cli_installer() {
        Some(cmd) => {
            let code = cli::stream(cmd, &on_line).map_err(|e| e.to_string())?;
            // pipx exits non-zero when the package is *already installed*, so
            // treat "matrix is present afterward" as success regardless of code.
            let ok = code == 0 || cli::cli_exists();
            if ok && code != 0 {
                let _ = on_line.send("matrix-cli is already installed.".into());
            }
            Ok(ok)
        }
        None => {
            let _ = on_line.send("No pipx or Python found. Install Python 3.11+ first.".into());
            Err("python not found".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install a component via matrix-cli (auto-installs the CLI first if missing).
#[tauri::command]
pub async fn install_component(
    entity: String,
    alias: Option<String>,
    hub: Option<String>,
    on_line: Channel<String>,
) -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if which("matrix").is_err() {
            let _ = on_line.send("matrix-cli not found — installing it first…".into());
            if let Some(cmd) = cli_installer() {
                let _ = cli::stream(cmd, &on_line);
            }
        }
        if which("matrix").is_err() {
            return Err("matrix-cli is not available after install".to_string());
        }
        let mut cmd = Command::new("matrix");
        cmd.arg("install").arg(&entity);
        if let Some(a) = alias.as_deref().filter(|a| !a.is_empty()) {
            cmd.arg("--alias").arg(a);
        }
        if let Some(h) = hub.as_deref().filter(|h| !h.is_empty()) {
            cmd.arg("--hub").arg(h);
            cmd.env("MATRIX_HUB_BASE", h);
        }
        cli::stream(cmd, &on_line).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Run an arbitrary `matrix …` command from the embedded terminal.
#[tauri::command]
pub async fn run_command(line: String, on_line: Channel<String>) -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut args = cli::split_args(&line);
        if args.is_empty() {
            return Ok(0);
        }
        // Allow either "matrix search …" or just "search …".
        if args[0] == "matrix" {
            args.remove(0);
        }
        if which("matrix").is_err() {
            let _ = on_line
                .send("matrix-cli is not installed. Run setup or click 'Install Matrix CLI'.".into());
            return Err("matrix-cli not found".to_string());
        }
        let mut cmd = Command::new("matrix");
        cmd.args(&args);
        cli::stream(cmd, &on_line).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
