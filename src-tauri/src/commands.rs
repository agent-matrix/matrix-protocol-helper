//! Tauri commands invoked by the MatrixHub Client frontend.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;


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

/// Bridge a log line from the frontend into the persistent diagnostics log.
/// This lets the UI record what it actually observes (PTY bytes received,
/// terminal lifecycle, render errors) so the diagnosis report captures the
/// *frontend's* view too — invaluable for debugging WebView-only issues that
/// never reach Rust. `level` is one of debug|info|warn|error (default info).
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    match level.as_str() {
        "debug" => cli::log_debug(&format!("[ui] {message}")),
        "warn" => cli::log_warn(&format!("[ui] {message}")),
        "error" => cli::log_error(&format!("[ui] {message}")),
        _ => cli::log_event(&format!("[ui] {message}")),
    }
}

/// Measure hub connectivity, returning round-trip milliseconds.
#[tauri::command]
pub async fn test_hub(url: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || cli::ping_hub(&url))
        .await
        .map_err(|e| e.to_string())?
}

/// Full snapshot of the managed Python `.venv` runtime: whether Python is
/// available, whether the venv and its `matrix` binary exist, the detected
/// versions, and an overall `ready` flag. Also written to the diagnostics log.
/// Use this to verify the backend is installed and the program is ready.
#[tauri::command]
pub async fn runtime_diagnostics() -> cli::RuntimeDiagnostics {
    tauri::async_runtime::spawn_blocking(cli::runtime_diagnostics)
        .await
        .unwrap_or_else(|_| cli::RuntimeDiagnostics {
            python_ok: false,
            python_version: None,
            venv_path: None,
            venv_python_ok: false,
            matrix_installed: false,
            matrix_path: None,
            matrix_version: None,
            ready: false,
        })
}

/// Find a real Python interpreter (skips the Windows Store alias stub).
fn find_python() -> Option<String> {
    cli::find_real_python()
}

/// Set up the app-managed runtime (create `.venv` + install matrix-cli into it).
/// Streams output, resolves true on success.
#[tauri::command]
pub async fn install_cli(on_line: Channel<String>) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || cli::ensure_runtime(&on_line))
        .await
        .map_err(|e| e.to_string())?
}

/// Install a component via matrix-cli (provisions the runtime first if missing).
#[tauri::command]
pub async fn install_component(
    entity: String,
    alias: Option<String>,
    hub: Option<String>,
    on_line: Channel<String>,
) -> Result<i32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !cli::cli_exists() {
            let _ = on_line.send("matrix-cli not found — setting up the runtime first…".into());
            let _ = cli::ensure_runtime(&on_line);
        }
        if !cli::cli_exists() {
            return Err("matrix-cli is not available after setup".to_string());
        }
        let mut cmd = cli::command(cli::matrix_program());
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

/// Run an arbitrary `matrix …` command using the managed runtime.
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
        if !cli::cli_exists() {
            let _ = on_line
                .send("matrix-cli is not installed. Run setup or click 'Install Matrix CLI'.".into());
            return Err("matrix-cli not found".to_string());
        }
        let mut cmd = cli::command(cli::matrix_program());
        cmd.args(&args);
        cli::stream(cmd, &on_line).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/* ============================================================
   Auto-update (premium, AAA-style flow)
   ============================================================ */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    available: bool,
    current_version: String,
    version: String,
    notes: Option<String>,
    date: Option<String>,
}

/// Check the configured update endpoint for a newer signed release.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => Ok(UpdateInfo {
            available: true,
            current_version: current,
            version: update.version.clone(),
            notes: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
        }),
        None => Ok(UpdateInfo {
            available: false,
            version: current.clone(),
            current_version: current,
            notes: None,
            date: None,
        }),
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
    pct: u32,
    phase: String,
}

/// Download + install the available update, streaming progress, then relaunch.
/// On success the process restarts, so this command does not return normally.
#[tauri::command]
pub async fn install_update(app: AppHandle, on_progress: Channel<DownloadProgress>) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;

    let downloaded = Arc::new(AtomicU64::new(0));
    let dl = downloaded.clone();
    let ch = on_progress.clone();
    let ch_done = on_progress.clone();

    update
        .download_and_install(
            move |chunk, total| {
                let d = dl.fetch_add(chunk as u64, Ordering::SeqCst) + chunk as u64;
                let pct = total
                    .map(|t| if t > 0 { ((d as f64 / t as f64) * 100.0) as u32 } else { 0 })
                    .unwrap_or(0);
                let _ = ch.send(DownloadProgress { downloaded: d, total, pct, phase: "download".into() });
            },
            move || {
                let _ = ch_done.send(DownloadProgress { downloaded: 0, total: None, pct: 100, phase: "install".into() });
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    // Relaunch into the freshly installed version.
    app.restart();
}

/* ============================================================
   Diagnostics / supportability
   ============================================================ */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    identifier: String,
    tauri_version: String,
    os: String,
    arch: String,
}

/// Basic app/build info for the About panel.
#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
        identifier: app.config().identifier.clone(),
        tauri_version: tauri::VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// One detected issue with a severity and an actionable fix. Surfaced at the
/// top of the diagnosis so a human or AI reviewer sees the verdict first.
struct Finding {
    severity: &'static str, // "BLOCKER" | "WARN" | "INFO"
    title: String,
    detail: String,
    fix: String,
}

/// Run all automated checks against the current runtime + PTY state and return
/// the findings, ordered by severity. This is the "brain" of the diagnosis:
/// every check knows what good looks like and what to do when it isn't.
fn detect_findings(
    diag: &cli::RuntimeDiagnostics,
    pty: &crate::pty::PtyStats,
    hub_ok: Option<bool>,
) -> Vec<Finding> {
    let mut f = Vec::new();

    if !diag.python_ok {
        f.push(Finding {
            severity: "BLOCKER",
            title: "No Python interpreter found".into(),
            detail: "A real Python 3.11+ is required to build the managed runtime.".into(),
            fix: "Install Python (Settings ▸ Install Python, or python.org) and relaunch so it joins PATH.".into(),
        });
    }
    if diag.python_ok && !diag.venv_python_ok {
        f.push(Finding {
            severity: "WARN",
            title: "Managed venv not created yet".into(),
            detail: "Python is available but the app-managed .venv does not exist.".into(),
            fix: "Click 'Install Matrix CLI' (or Reset Matrix CLI) to provision the runtime.".into(),
        });
    }
    if diag.venv_python_ok && !diag.matrix_installed {
        f.push(Finding {
            severity: "BLOCKER",
            title: "matrix-cli is not installed in the venv".into(),
            detail: "The venv exists but its `matrix` executable is missing.".into(),
            fix: "Run Reset Matrix CLI to reinstall, then re-run this diagnosis.".into(),
        });
    }
    if hub_ok == Some(false) {
        f.push(Finding {
            severity: "WARN",
            title: "MatrixHub is unreachable".into(),
            detail: "Could not open a TCP connection to the configured hub.".into(),
            fix: "Check the Hub URL in Settings and your network/proxy/VPN.".into(),
        });
    }
    // Terminal-specific heuristics — these catch the exact class of bug seen on
    // Windows (PTY opens but the visible terminal shows nothing).
    if pty.opened >= 2 && pty.bytes_streamed == 0 {
        f.push(Finding {
            severity: "BLOCKER",
            title: "Terminal opened but produced no output".into(),
            detail: format!(
                "{} PTY sessions were opened but 0 bytes were ever streamed back. The shell is not emitting a prompt.",
                pty.opened
            ),
            fix: "Check the shell path in the log ('opening PTY shell'). On Windows ensure PowerShell exists; try Reset and relaunch.".into(),
        });
    }
    if pty.opened >= 6 && pty.currently_open <= 1 {
        f.push(Finding {
            severity: "INFO",
            title: "Many short-lived terminal sessions".into(),
            detail: format!(
                "{} PTYs opened, {} closed — typical of repeated view switches or a remount loop.",
                pty.opened, pty.closed
            ),
            fix: "Harmless if the terminal works. If it flickers, report this section.".into(),
        });
    }

    let rank = |s: &str| match s {
        "BLOCKER" => 0,
        "WARN" => 1,
        _ => 2,
    };
    f.sort_by_key(|x| rank(x.severity));
    f
}

/// Generate a complete, self-contained diagnosis report for debugging.
///
/// This is an industrial-strength report designed to make *future, unknown*
/// errors debuggable without a live reproduction. It bundles:
///   • an automated verdict with severity-ranked findings and concrete fixes;
///   • build/platform info and a redacted environment snapshot;
///   • a full snapshot of the managed Python `.venv` runtime;
///   • live tool probes (`matrix --version`, `matrix --help`);
///   • terminal/PTY health counters (opens, closes, bytes streamed);
///   • hub connectivity;
///   • the most recent ERROR/WARN lines, then the raw log tail.
/// The Markdown is meant to be pasted into a bug report or handed to an AI
/// assistant so it can fix the current application state.
#[tauri::command]
pub async fn generate_diagnosis(app: AppHandle, hub_url: Option<String>) -> Result<String, String> {
    let pkg_name = app.package_info().name.clone();
    let pkg_version = app.package_info().version.to_string();
    let identifier = app.config().identifier.clone();
    let log_path = cli::log_path().map(|p| p.display().to_string());

    tauri::async_runtime::spawn_blocking(move || {
        let diag = cli::runtime_diagnostics();
        let pty = crate::pty::stats();
        cli::log_event("generate_diagnosis: building report");

        // Hub probe.
        let (hub_ok, hub_line) = match hub_url.as_deref() {
            Some(url) if !url.is_empty() => match cli::ping_hub(url) {
                Ok(ms) => (Some(true), format!("- Hub `{url}`: reachable ({ms} ms)")),
                Err(e) => (Some(false), format!("- Hub `{url}`: UNREACHABLE ({e})")),
            },
            _ => (None, "- Hub: not tested".to_string()),
        };

        let findings = detect_findings(&diag, &pty, hub_ok);
        let yn = |b: bool| if b { "yes" } else { "NO" };

        // --- Verdict block (top of report) ---
        let blockers = findings.iter().filter(|f| f.severity == "BLOCKER").count();
        let warns = findings.iter().filter(|f| f.severity == "WARN").count();
        let verdict = if blockers > 0 {
            format!("❌ NOT READY — {blockers} blocker(s), {warns} warning(s)")
        } else if warns > 0 {
            format!("⚠️  READY WITH WARNINGS — {warns} warning(s)")
        } else if diag.ready {
            "✅ HEALTHY — all checks passed".to_string()
        } else {
            "⚠️  UNKNOWN — runtime not ready".to_string()
        };

        let mut findings_md = String::new();
        if findings.is_empty() {
            findings_md.push_str("_No issues detected by automated checks._\n");
        } else {
            for fnd in &findings {
                findings_md.push_str(&format!(
                    "- **[{}] {}**\n  - {}\n  - Fix: {}\n",
                    fnd.severity, fnd.title, fnd.detail, fnd.fix
                ));
            }
        }

        // --- Live tool probes ---
        let probe = |label: &str, args: &[&str]| -> String {
            if !diag.ready {
                return format!("- `matrix {}`: skipped (CLI not installed)", args.join(" "));
            }
            match cli::capture(&cli::matrix_program(), args, 1200) {
                Ok((code, out)) => format!(
                    "- `matrix {}` (exit {code}):\n```\n{}\n```",
                    args.join(" "),
                    if out.is_empty() { "(no output)".to_string() } else { out }
                ),
                Err(e) => format!("- `matrix {}`: could not run ({e})  [{label}]", args.join(" ")),
            }
        };
        let probe_version = probe("version", &["--version"]);
        // `--help` is a clean, always-valid probe that confirms the CLI fully
        // loads its command tree (unlike `doctor`, which requires an ALIAS arg).
        let probe_help = probe("help", &["--help"]);

        // --- Environment snapshot ---
        let mut env_md = String::new();
        for (k, v) in cli::env_snapshot() {
            env_md.push_str(&format!("- {k}: `{v}`\n"));
        }
        if env_md.is_empty() {
            env_md.push_str("- (no relevant environment variables set)\n");
        }

        // --- Recent problems + raw tail ---
        let problems = cli::recent_problems(40);
        let problems_md = if problems.is_empty() {
            "_No ERROR/WARN lines in the log._".to_string()
        } else {
            format!("```\n{}\n```", problems.join("\n"))
        };
        let tail = cli::log_tail(200);
        let tail = if tail.is_empty() { "(log is empty or unavailable)".to_string() } else { tail };

        let now = cli::now_utc();

        let report = format!(
            "# MatrixHub Client — Diagnosis Report\n\
             \n\
             > Generated: {now}\n\
             > Paste this entire report into a bug report or hand it to an AI assistant to debug the app.\n\
             \n\
             ## Verdict\n\
             **{verdict}**\n\
             \n\
             ## Findings (automated checks)\n\
             {findings_md}\n\
             ## Application\n\
             - Product: {pkg_name}\n\
             - Version: v{pkg_version}\n\
             - Identifier: {identifier}\n\
             - Tauri: {tauri}\n\
             - OS / Arch: {os} / {arch}\n\
             - Log file: {logp}\n\
             \n\
             ## Runtime readiness\n\
             - **Ready to work: {ready}**\n\
             - Python available: {py_ok} ({py_ver})\n\
             - Managed venv path: {venv}\n\
             - venv Python present: {venv_py}\n\
             - matrix-cli installed: {mtx_ok}\n\
             - matrix path: {mtx_path}\n\
             - matrix version: {mtx_ver}\n\
             \n\
             ## Terminal / PTY health\n\
             - Sessions opened (lifetime): {pty_open}\n\
             - Sessions closed (lifetime): {pty_close}\n\
             - Currently open: {pty_now}\n\
             - Total bytes streamed from shells: {pty_bytes}\n\
             \n\
             ## Live tool probes\n\
             {probe_version}\n\
             {probe_help}\n\
             \n\
             ## Connectivity\n\
             {hub_line}\n\
             \n\
             ## Environment\n\
             {env_md}\n\
             ## Recent problems (ERROR / WARN)\n\
             {problems_md}\n\
             \n\
             ## Recent log (last 200 lines of client.log)\n\
             ```\n{tail}\n```\n",
            tauri = tauri::VERSION,
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            logp = log_path.as_deref().unwrap_or("(unset)"),
            ready = yn(diag.ready),
            py_ok = yn(diag.python_ok),
            py_ver = diag.python_version.as_deref().unwrap_or("-"),
            venv = diag.venv_path.as_deref().unwrap_or("(unset)"),
            venv_py = yn(diag.venv_python_ok),
            mtx_ok = yn(diag.matrix_installed),
            mtx_path = diag.matrix_path.as_deref().unwrap_or("-"),
            mtx_ver = diag.matrix_version.as_deref().unwrap_or("-"),
            pty_open = pty.opened,
            pty_close = pty.closed,
            pty_now = pty.currently_open,
            pty_bytes = pty.bytes_streamed,
        );

        // Persist the report next to the logs so it survives even if the
        // clipboard copy fails, and log that we produced it.
        if let Some(dir) = cli::log_path().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let file = dir.join(format!("diagnosis-{stamp}.md"));
            if std::fs::write(&file, &report).is_ok() {
                cli::log_event(&format!("generate_diagnosis: saved {}", file.display()));
            }
        }
        cli::log_event(&format!(
            "generate_diagnosis: done — {blockers} blocker(s), {warns} warning(s)"
        ));

        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Repair the Matrix CLI: delete the managed runtime and recreate it cleanly.
#[tauri::command]
pub async fn reset_cli(on_line: Channel<String>) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _ = on_line.send("Removing the managed runtime…".into());
        if let Err(e) = cli::remove_runtime() {
            let _ = on_line.send(format!("(could not fully remove: {e})"));
        }
        cli::ensure_runtime(&on_line)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open the client's data folder in the OS file manager.
#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.to_string_lossy().to_string();
    reveal_path(&path)?;
    Ok(path)
}

/// Write the in-app log buffer to a timestamped file and reveal it.
#[tauri::command]
pub fn export_logs(app: AppHandle, content: String) -> Result<String, String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = dir.join(format!("matrixhub-client-{stamp}.log"));
    std::fs::write(&file, content).map_err(|e| e.to_string())?;
    let path = file.to_string_lossy().to_string();
    let _ = reveal_path(&dir.to_string_lossy());
    Ok(path)
}

/// Open a path in the platform file manager (no console window on Windows).
fn reveal_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = cli::command("explorer");
    #[cfg(target_os = "macos")]
    let mut cmd = cli::command("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = cli::command("xdg-open");
    cmd.arg(path);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/* ============================================================
   Python preflight + helpers
   ============================================================ */

/// Install Python 3 using the platform's standard package manager.
/// Windows → winget, macOS → Homebrew, Linux → guidance. Falls back to
/// guidance + the python.org link when no package manager is available.
#[tauri::command]
pub async fn install_python(on_line: Channel<String>) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if find_python().is_some() {
            let _ = on_line.send("Python is already installed.".into());
            return Ok(true);
        }

        #[cfg(target_os = "windows")]
        {
            if which::which("winget").is_ok() {
                let _ = on_line.send("Installing Python 3.12 via winget…".into());
                let mut c = cli::command("winget");
                c.args([
                    "install", "-e", "--id", "Python.Python.3.12",
                    "--silent", "--accept-package-agreements", "--accept-source-agreements",
                ]);
                let _ = cli::stream(c, &on_line);
            } else {
                let _ = on_line.send(
                    "winget was not found. Download Python from https://www.python.org/downloads/ (enable 'Add python.exe to PATH').".into(),
                );
            }
        }
        #[cfg(target_os = "macos")]
        {
            if which::which("brew").is_ok() {
                let _ = on_line.send("Installing Python via Homebrew…".into());
                let mut c = cli::command("brew");
                c.args(["install", "python"]);
                let _ = cli::stream(c, &on_line);
            } else {
                let _ = on_line.send(
                    "Homebrew was not found. Download Python from https://www.python.org/downloads/ or install Homebrew first.".into(),
                );
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = on_line.send(
                "Install Python with your package manager, e.g.\n  sudo apt install -y python3 python3-pip python3-venv\n  sudo dnf install -y python3 python3-pip".into(),
            );
        }

        // PATH may not refresh in this process until the app is restarted.
        let ok = find_python().is_some();
        if !ok {
            let _ = on_line.send("If Python was just installed, restart MatrixHub Client so it appears on PATH.".into());
        }
        Ok(ok)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open an http(s) URL in the user's default browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = cli::command("cmd");
        c.args(["/C", "start", "", &url]);
        c.spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        cli::command("open").arg(&url).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        cli::command("xdg-open").arg(&url).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Relaunch the application (used after installing Python so PATH refreshes).
#[tauri::command]
pub fn relaunch(app: AppHandle) {
    app.restart();
}
