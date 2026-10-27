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
//!   `downloads_cancel` (B2.3: `downloads_remove`): the fixed download operations, validated in [`downloads`];
//! - B2.2 `desktop_settings_get`, `desktop_settings_set`, `tray_set_labels`, `service_exit_and_stop`:
//!   close behaviour, tray labels and the confirmed "Exit and stop downloads" ([`lifecycle`]).
//!
//! Event forwarding arrives later (ARCH-08); the UI polls `downloads_list` until then.

mod downloads;
mod lifecycle;
mod service;
#[cfg(feature = "smoke")]
mod smoke;

use dm_domain::api::{DownloadSnapshot, ListDownloadsResult};
use downloads::{BridgeError, Control};
use lifecycle::{CloseBehaviour, ExitReport, Lifecycle, SettingsView};

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

/// Removes a finished download from the list (ADR 0013): never cancels, never deletes files.
#[tauri::command]
async fn downloads_remove(job_id: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::control(&data_dir()?, Control::Remove, &job_id).await
}

/// The window's close setting and whether the tray exists (Options).
#[tauri::command]
fn desktop_settings_get(state: tauri::State<'_, Lifecycle<tauri::Wry>>) -> SettingsView {
    state.view()
}

/// Persists the close setting. The value is the closed enum; anything else is refused by type.
#[tauri::command]
fn desktop_settings_set(
    close_behaviour: CloseBehaviour,
    state: tauri::State<'_, Lifecycle<tauri::Wry>>,
) -> Result<SettingsView, BridgeError> {
    state.set_close_behaviour(close_behaviour)
}

/// The tray menu's labels, from the UI's English/Bangla catalogue.
#[tauri::command]
fn tray_set_labels(
    open: String,
    exit: String,
    state: tauri::State<'_, Lifecycle<tauri::Wry>>,
) -> Result<(), BridgeError> {
    state.set_labels(&open, &exit)
}

/// The confirmed "Exit and stop downloads": pauses and stops the service, verifies it has gone,
/// then quits the desktop. The UI calls this only after its confirmation dialog.
#[tauri::command]
async fn service_exit_and_stop(app: tauri::AppHandle) -> Result<ExitReport, BridgeError> {
    let report = lifecycle::exit_and_stop(&app).await;
    #[cfg(feature = "smoke")]
    smoke::lifecycle_event(
        "exit_and_stop",
        match &report {
            Ok(r) => serde_json::json!({ "report": r }),
            Err(e) => serde_json::json!({ "error": e }),
        },
    );
    if let Ok(ExitReport { stopped: true, .. }) = report {
        let quit = app.clone();
        tauri::async_runtime::spawn(async move {
            // Let the answer reach the window first.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            quit.exit(0);
        });
    }
    report
}

/// Builds and runs the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (for example, a missing system webview).
pub fn run() {
    let builder = tauri::Builder::default()
        // First, so a second launch is caught before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            lifecycle::show_main(app);
            #[cfg(feature = "smoke")]
            smoke::lifecycle_event("second_instance", smoke::window_state(app));
        }))
        .manage(Lifecycle::<tauri::Wry>::new(
            dm_platform::paths::data_dir().ok(),
        ))
        .setup(|app| {
            lifecycle::create_tray(app.handle());
            #[cfg(feature = "smoke")]
            smoke::exit_driver(app.handle());
            #[cfg(feature = "smoke")]
            smoke::close_driver(app.handle());
            #[cfg(feature = "smoke")]
            smoke::command_driver(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                let _action = lifecycle::on_close_requested(window, api);
                #[cfg(feature = "smoke")]
                smoke::lifecycle_event("close_requested", serde_json::json!({ "action": _action }));
            }
        })
        .invoke_handler(tauri::generate_handler![
            service_connect,
            downloads_list,
            downloads_add,
            downloads_pause,
            downloads_resume,
            downloads_cancel,
            downloads_remove,
            desktop_settings_get,
            desktop_settings_set,
            tray_set_labels,
            service_exit_and_stop
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
