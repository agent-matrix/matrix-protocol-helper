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

/// Build the best-available CLI installer command (pipx preferred, then pip).
fn cli_installer() -> Option<Command> {
    if which("pipx").is_ok() {
        let mut c = Command::new("pipx");
        c.args(["install", "matrix-cli"]);
        Some(c)
    } else if which("python3").is_ok() {
        let mut c = Command::new("python3");
        c.args(["-m", "pip", "install", "--user", "matrix-cli"]);
        Some(c)
    } else if which("python").is_ok() {
        let mut c = Command::new("python");
        c.args(["-m", "pip", "install", "--user", "matrix-cli"]);
        Some(c)
    } else {
        None
    }
}

/// Install the Matrix CLI; streams output, resolves true on success.
#[tauri::command]
pub async fn install_cli(on_line: Channel<String>) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || match cli_installer() {
        Some(cmd) => {
            let code = cli::stream(cmd, &on_line).map_err(|e| e.to_string())?;
            Ok(code == 0)
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
