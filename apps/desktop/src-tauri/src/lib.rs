//! Desktop shell.
//!
//! The shell presents service state and forwards user commands. It never downloads file bytes,
//! never talks to aria2, and never opens SQLite: those belong to `dm-service`
//! (see docs/architecture.md). Closing the window does not stop downloads: the service runs as
//! its own detached process; the shell will only *ask* it to stop on an explicit "Exit and stop
//! downloads".
//!
//! Commands (each granted explicitly in `capabilities/default.json`, SEC-05):
//! - B0.5 `service_connect`: find or start the packaged service and return its `hello`;
//! - B2.1 `downloads_list`, `downloads_add`, `downloads_pause`, `downloads_resume`,
//!   `downloads_cancel`: the fixed download operations, validated in [`downloads`].
//!
//! Event forwarding arrives later (ARCH-08); the UI polls `downloads_list` until then.

mod downloads;
mod service;
#[cfg(feature = "smoke")]
mod smoke;

use dm_domain::api::{DownloadSnapshot, ListDownloadsResult};
use downloads::{BridgeError, Control};

/// Finds or starts the per-user download service; returns a UI `ConnectionState`.
#[tauri::command]
async fn service_connect() -> serde_json::Value {
    service::connect().await
}

fn data_dir() -> Result<std::path::PathBuf, BridgeError> {
    dm_platform::paths::data_dir().map_err(|_| BridgeError::no_data_dir())
}

/// The service's current download snapshots.
#[tauri::command]
async fn downloads_list() -> Result<ListDownloadsResult, BridgeError> {
    downloads::list(&data_dir()?).await
}

/// Adds one http(s) link. A retry of an uncertain add must reuse `idempotency_key`.
#[tauri::command]
async fn downloads_add(
    url: String,
    idempotency_key: String,
) -> Result<DownloadSnapshot, BridgeError> {
    downloads::add(&data_dir()?, url, idempotency_key).await
}

/// Manual pause (`desired_state = paused`).
#[tauri::command]
async fn downloads_pause(job_id: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::control(&data_dir()?, Control::Pause, &job_id).await
}

/// Resume of a paused job (refused when partial data cannot be validated).
#[tauri::command]
async fn downloads_resume(job_id: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::control(&data_dir()?, Control::Resume, &job_id).await
}

/// Cancel; acknowledged after the engine stopped.
#[tauri::command]
async fn downloads_cancel(job_id: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::control(&data_dir()?, Control::Cancel, &job_id).await
}

/// Builds and runs the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (for example, a missing system webview).
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![
        service_connect,
        downloads_list,
        downloads_add,
        downloads_pause,
        downloads_resume,
        downloads_cancel
    ]);
    #[cfg(feature = "smoke")]
    let builder = builder.on_page_load(|webview, payload| {
        use tauri::Manager as _;
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
            smoke::report(webview.app_handle());
            smoke::bridge(webview);
        }
    });
    builder
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri runtime");
}
