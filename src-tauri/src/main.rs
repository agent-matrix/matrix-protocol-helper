// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod cli;
mod commands;
mod pty;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use url::Url;

/// A validated `matrix://install` request, forwarded to the frontend so the
/// MatrixHub Client can show its in-app install-approval modal.
#[derive(Clone, Serialize)]
struct InstallRequest {
    entity: String,
    alias: Option<String>,
    hub: Option<String>,
}

fn parse_and_sanitize_link(link: &str) -> Result<InstallRequest, String> {
    let url = Url::parse(link).map_err(|_| "Invalid URL format".to_string())?;
    if url.scheme() != "matrix" {
        return Err("URL scheme must be 'matrix://'".to_string());
    }
    if url.host_str() != Some("install") {
        return Err("Only 'matrix://install' action is supported".to_string());
    }
    // The entity id. matrixhub.io emits both `entity=` (InstallWithCli) and
    // `id=` (ResultCard / InstallCTA) — accept either so every Install button
    // on the marketplace works. `entity` wins if both are present.
    let entity = url
        .query_pairs()
        .find(|(k, _)| k == "entity")
        .or_else(|| url.query_pairs().find(|(k, _)| k == "id"))
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| "Required parameter 'entity' (or 'id') is missing".to_string())?;
    // Alias is OPTIONAL: matrixhub.io's InstallCTA omits it, and matrix-cli
    // auto-suggests a friendly alias when none is passed. When present we still
    // validate it strictly.
    let alias = url
        .query_pairs()
        .find(|(k, _)| k == "alias")
        .map(|(_, v)| v.to_string())
        .filter(|a| !a.is_empty());
    let hub = url
        .query_pairs()
        .find(|(k, _)| k == "hub")
        .map(|(_, v)| v.to_string());
    if entity.is_empty() || entity.len() > 256 {
        return Err("Parameter 'entity' is invalid".to_string());
    }
    if let Some(a) = &alias {
        if a.len() > 64 {
            return Err("Parameter 'alias' is invalid".to_string());
        }
        if !a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Alias contains invalid characters. Use only A-Z, a-z, 0-9, -, _".to_string(),
            );
        }
    }
    if let Some(h) = &hub {
        if !h.starts_with("http://") && !h.starts_with("https://") {
            return Err("Hub must be a valid http or https URL".to_string());
        }
    }
    Ok(InstallRequest { entity, alias, hub })
}

/// Brings the main client window to the foreground.
fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Handles a `matrix://` deep link: validate, focus the client, and hand the
/// request to the frontend, which renders the install-approval modal and runs
/// the install through the `install_component` command.
fn handle_link_request(app: &AppHandle, link: &str) {
    match parse_and_sanitize_link(link) {
        Ok(req) => {
            focus_main(app);
            // The frontend listens for this on startup.
            let _ = app.emit("install-request", req);
        }
        Err(msg) => {
            app.dialog()
                .message(msg)
                .title("Invalid Link")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_entity_param() {
        let r = parse_and_sanitize_link(
            "matrix://install?entity=mcp.io-github-x.stdio.abc@1.0.1&alias=watsonx",
        )
        .unwrap();
        assert_eq!(r.entity, "mcp.io-github-x.stdio.abc@1.0.1");
        assert_eq!(r.alias.as_deref(), Some("watsonx"));
        assert!(r.hub.is_none());
    }

    #[test]
    fn accepts_id_param_from_matrixhub_resultcard() {
        // matrixhub.io ResultCard / InstallCTA emit `id=` instead of `entity=`.
        let r = parse_and_sanitize_link("matrix://install?id=tool.io-github-x.abc@1.0.1&alias=my-tool")
            .unwrap();
        assert_eq!(r.entity, "tool.io-github-x.abc@1.0.1");
        assert_eq!(r.alias.as_deref(), Some("my-tool"));
    }

    #[test]
    fn entity_wins_when_both_present() {
        let r = parse_and_sanitize_link("matrix://install?entity=AAA&id=BBB&alias=a").unwrap();
        assert_eq!(r.entity, "AAA");
    }

    #[test]
    fn alias_is_optional() {
        // matrixhub.io InstallCTA omits alias entirely.
        let r = parse_and_sanitize_link("matrix://install?id=tool:x@1.0.0").unwrap();
        assert_eq!(r.entity, "tool:x@1.0.0");
        assert!(r.alias.is_none());
        // An explicitly empty alias is also treated as absent.
        let r2 = parse_and_sanitize_link("matrix://install?id=tool:x@1.0.0&alias=").unwrap();
        assert!(r2.alias.is_none());
    }

    #[test]
    fn rejects_wrong_scheme_and_host() {
        assert!(parse_and_sanitize_link("https://install?id=x").is_err());
        assert!(parse_and_sanitize_link("matrix://run?id=x").is_err());
    }

    #[test]
    fn rejects_missing_entity_and_id() {
        assert!(parse_and_sanitize_link("matrix://install?alias=a").is_err());
    }

    #[test]
    fn rejects_alias_injection_and_overlong() {
        // Shell metacharacters / spaces are rejected (defense in depth — the
        // alias is also never passed through a shell).
        assert!(parse_and_sanitize_link("matrix://install?id=x&alias=a;rm%20-rf%20/").is_err());
        assert!(parse_and_sanitize_link("matrix://install?id=x&alias=a%20b").is_err());
        let long = "a".repeat(65);
        assert!(parse_and_sanitize_link(&format!("matrix://install?id=x&alias={long}")).is_err());
    }

    #[test]
    fn validates_hub_override() {
        let ok =
            parse_and_sanitize_link("matrix://install?id=x&alias=a&hub=https://api.matrixhub.io")
                .unwrap();
        assert_eq!(ok.hub.as_deref(), Some("https://api.matrixhub.io"));
        assert!(parse_and_sanitize_link("matrix://install?id=x&alias=a&hub=ftp://evil").is_err());
    }

    #[test]
    fn rejects_overlong_entity() {
        let long = "a".repeat(257);
        assert!(parse_and_sanitize_link(&format!("matrix://install?id={long}&alias=a")).is_err());
    }

    #[test]
    fn split_args_handles_quotes() {
        assert_eq!(cli::split_args("matrix search github"), vec!["matrix", "search", "github"]);
        assert_eq!(
            cli::split_args("search \"voice agent\" --json"),
            vec!["search", "voice agent", "--json"]
        );
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::cli_status,
            commands::test_hub,
            commands::runtime_diagnostics,
            commands::install_cli,
            commands::install_component,
            commands::run_command,
            commands::check_update,
            commands::install_update,
            commands::app_info,
            commands::reset_cli,
            commands::open_data_dir,
            commands::export_logs,
            commands::install_python,
            commands::open_url,
            commands::relaunch,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
        ])
        .setup(|app| {
            // Persistent diagnostics log: <app_log_dir>/client.log
            if let Ok(dir) = app.path().app_log_dir() {
                let _ = std::fs::create_dir_all(&dir);
                cli::set_log_path(dir.join("client.log"));
                cli::log_event("=== MatrixHub Client started ===");
            }
            // App-managed Python runtime: <app_data_dir>/runtime/.venv
            if let Ok(dir) = app.path().app_data_dir() {
                cli::set_runtime_dir(dir.join("runtime"));
            }

            // Log build/platform info and a full runtime snapshot at startup so
            // every session's log begins with the state needed to debug it.
            cli::log_event(&format!(
                "build v{} on {} {}",
                app.package_info().version,
                std::env::consts::OS,
                std::env::consts::ARCH,
            ));
            let _ = cli::runtime_diagnostics();

            let handle = app.handle().clone();

            // matrix:// deep links arrive as new-instance events.
            app.listen("deep-link://new-instance", move |event| {
                let link_str = event.payload();
                if let Ok(link) = serde_json::from_str::<String>(link_str) {
                    handle_link_request(&handle, &link);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
