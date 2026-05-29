// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod cli;

use cli::{cli_exists, run_matrix_install_stream};
// Import traits to bring extension methods into scope.
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use url::Url;

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

/// Shows the progress window and brings it to the front.
fn show_progress_window(app: &AppHandle, title: &str) {
    if let Some(win) = app.get_webview_window("progress") {
        let _ = win.set_title(title);
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// The main logic for handling a deep link request.
fn handle_link_request(app: &AppHandle, link: &str) {
    // 1. Parse and sanitize the link.
    let request = match parse_and_sanitize_link(link) {
        Ok(req) => req,
        Err(msg) => {
            app.dialog()
                .message(msg)
                .title("Invalid Link")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
            return;
        }
    };

    // 2. Check if the Matrix CLI is installed.
    if !cli_exists() {
        app.dialog()
            .message("Matrix CLI is not installed or not in your PATH.\n\nPlease install it with:\n\tpipx install matrix-cli\n\nThen click the install link again.")
            .title("Matrix CLI Not Found")
            .kind(MessageDialogKind::Warning)
            .show(|_| {});
        
        // NOTE: `open_url` takes `Option<impl Into<String>>`; annotate `None` to avoid E0283.
        let _ = app
            .opener()
            .open_url("https://pypi.org/project/matrix-cli/", None::<&str>);
        return;
    }

    // 3. Ask the user for confirmation (Rust-side dialog with Yes/No).
    let app_handle = app.clone();
    let alias_display = request
        .alias
        .clone()
        .unwrap_or_else(|| "(auto — chosen by matrix-cli)".to_string());
    let confirmation_message = format!(
        "Do you want to install the following component?\n\nEntity:\n  {}\n\nAlias:\n  {}{}",
        request.entity,
        alias_display,
        request
            .hub
            .as_ref()
            .map(|h| format!("\n\nHub Override:\n  {}", h))
            .unwrap_or_default()
    );

    // In Rust, use .message(...).buttons(...).show(...) for a Yes/No dialog.
    app.dialog()
        .message(confirmation_message)
        .title("Confirm Installation")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Yes".into(), "No".into()))
        .show(move |yes| {
            if !yes {
                return; // User clicked "No".
            }

            // 4. User confirmed. Show progress and run command.
            show_progress_window(&app_handle, &format!("Installing '{}'...", alias_display));

            std::thread::spawn(move || {
                let alias_clone = alias_display.clone();
                match run_matrix_install_stream(
                    &app_handle,
                    &request.entity,
                    request.alias.as_deref(),
                    request.hub.as_deref(),
                ) {
                    Ok(code) => {
                        let _ = app_handle.emit(
                            "install-complete",
                            serde_json::json!({
                                "ok": code == 0,
                                "code": code,
                                "alias": alias_clone,
                            }),
                        );
                    }
                    Err(e) => {
                        let _ = app_handle.emit("log-line", format!("FATAL ERROR: {}", e));
                        let _ = app_handle.emit(
                            "install-complete",
                            serde_json::json!({
                                "ok": false,
                                "code": -1,
                                "alias": alias_clone,
                            }),
                        );
                    }
                }
            });
        });
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
        let r = parse_and_sanitize_link(
            "matrix://install?id=tool.io-github-x.abc@1.0.1&alias=my-tool",
        )
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
        let ok = parse_and_sanitize_link(
            "matrix://install?id=x&alias=a&hub=https://api.matrixhub.io",
        )
        .unwrap();
        assert_eq!(ok.hub.as_deref(), Some("https://api.matrixhub.io"));
        assert!(parse_and_sanitize_link("matrix://install?id=x&alias=a&hub=ftp://evil").is_err());
    }

    #[test]
    fn rejects_overlong_entity() {
        let long = "a".repeat(257);
        assert!(parse_and_sanitize_link(&format!("matrix://install?id={long}&alias=a")).is_err());
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Keep the same event-listener structure you provided.
            app.listen("deep-link://new-instance", move |event| {
                let link_str = event.payload();
                if let Ok(link) = serde_json::from_str::<String>(link_str) {
                    handle_link_request(&handle, &link);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
