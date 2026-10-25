//! Desktop shell.
//!
//! The shell presents service state and forwards user commands. It never downloads file bytes,
//! never talks to aria2, and never opens SQLite: those belong to `dm-service`
//! (see docs/architecture.md). Closing the window does not stop downloads: the service runs as
//! its own detached process; the shell will only *ask* it to stop on an explicit "Exit and stop
//! downloads".
//!
//! B0.5: one narrow command, `service_connect` (find or start the packaged service and return its
//! `hello`). The download commands and event forwarding arrive with B2.

mod service;

/// Finds or starts the per-user download service; returns a UI `ConnectionState`.
#[tauri::command]
async fn service_connect() -> serde_json::Value {
    service::connect().await
}

/// Test builds only (`--features smoke`): after the main window has loaded, write the window and
/// service handshake result to `$DOWNOVA_SMOKE_FILE` for the packaged-app smoke test.
#[cfg(feature = "smoke")]
fn smoke_report(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(path) = std::env::var_os("DOWNOVA_SMOKE_FILE") else {
        return;
    };
    let window = app.get_webview_window("main").map(|w| {
        (
            w.title().unwrap_or_default(),
            w.is_visible().unwrap_or(false),
        )
    });
    tauri::async_runtime::spawn(async move {
        let service = service::connect().await;
        let report = serde_json::json!({
            "window": window.as_ref().map(|(title, visible)| serde_json::json!({"title": title, "visible": visible})),
            "page_loaded": true,
            "service": service,
            "sidecar": service::sidecar_path().map(|p| p.display().to_string()),
            "pid": std::process::id(),
        });
        let _ = std::fs::write(path, report.to_string());
    });
}

/// Builds and runs the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (for example, a missing system webview).
pub fn run() {
    let builder =
        tauri::Builder::default().invoke_handler(tauri::generate_handler![service_connect]);
    #[cfg(feature = "smoke")]
    let builder = builder.on_page_load(|webview, payload| {
        use tauri::Manager as _;
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
            smoke_report(webview.app_handle());
        }
    });
    builder
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri runtime");
}
