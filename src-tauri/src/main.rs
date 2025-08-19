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
    alias: String,
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
    let entity = url
        .query_pairs()
        .find(|(k, _)| k == "entity")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| "Required parameter 'entity' is missing".to_string())?;
    let alias = url
        .query_pairs()
        .find(|(k, _)| k == "alias")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| "Required parameter 'alias' is missing".to_string())?;
    let hub = url
        .query_pairs()
        .find(|(k, _)| k == "hub")
        .map(|(_, v)| v.to_string());
    if entity.is_empty() || entity.len() > 256 {
        return Err("Parameter 'entity' is invalid".to_string());
    }
    if alias.is_empty() || alias.len() > 64 {
        return Err("Parameter 'alias' is invalid".to_string());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Alias contains invalid characters. Use only A-Z, a-z, 0-9, -, _".to_string());
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
    let confirmation_message = format!(
        "Do you want to install the following component?\n\nEntity:\n  {}\n\nAlias:\n  {}{}",
        request.entity,
        request.alias,
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
            show_progress_window(&app_handle, &format!("Installing '{}'...", request.alias));

            std::thread::spawn(move || {
                let alias_clone = request.alias.clone();
                match run_matrix_install_stream(
                    &app_handle,
                    &request.entity,
                    &request.alias,
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
