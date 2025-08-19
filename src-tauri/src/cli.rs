use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
// Emitter trait provides the .emit() method for AppHandle.
use tauri::Emitter;
use which::which;

/// Checks if the `matrix` CLI command exists in the system's PATH.
pub fn cli_exists() -> bool {
    which("matrix").is_ok()
}

/// Spawns `matrix install <entity> --alias <alias>` safely without a shell.
pub fn run_matrix_install_stream(
    app: &tauri::AppHandle,
    entity: &str,
    alias: &str,
    hub: Option<&str>,
) -> std::io::Result<i32> {
    let mut cmd = Command::new("matrix");
    cmd.arg("install")
        .arg(entity)
        .arg("--alias")
        .arg(alias)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(h) = hub {
        cmd.env("MATRIX_HUB_BASE", h);
    }

    let mut child = cmd.spawn()?;
    let mut threads: Vec<JoinHandle<()>> = vec![];

    if let Some(stdout) = child.stdout.take() {
        let app_handle = app.clone();
        let stdout_thread = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(l) = line {
                    // This now compiles because the Emitter trait is in scope.
                    let _ = app_handle.emit("log-line", l);
                }
            }
        });
        threads.push(stdout_thread);
    }

    if let Some(stderr) = child.stderr.take() {
        let app_handle = app.clone();
        let stderr_thread = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    // This now compiles because the Emitter trait is in scope.
                    let _ = app_handle.emit("log-line", l);
                }
            }
        });
        threads.push(stderr_thread);
    }

    let status = child.wait()?;
    for thread in threads {
        thread.join().unwrap();
    }

    Ok(status.code().unwrap_or(-1))
}
