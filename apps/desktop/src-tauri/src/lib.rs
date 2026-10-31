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
//! B2.5: on launch, [`browser_setup`] registers the installed native-messaging host for the current
//! user (once per install location and version) by running the sibling `dm-browser-host`.
//!
//! Event forwarding arrives later (ARCH-08); the UI polls `downloads_list` until then.

mod browser_setup;
mod downloads;
mod lifecycle;
mod service;
#[cfg(feature = "smoke")]
mod smoke;
mod stream;

use dm_domain::api::{
    CheckLinkResult, DownloadSnapshot, EmptyResult, ListDownloadsResult, ListQueuesResult,
    QueueInfo, ServiceSettings,
};
use downloads::{AddOptions, BridgeError, Control};
use lifecycle::{CloseBehaviour, ExitReport, Lifecycle, SettingsView};

/// Finds or starts the per-user download service; returns a UI `ConnectionState`. Once "Exit
/// and stop downloads" began it only looks, never starts (B2.10).
#[tauri::command]
async fn service_connect(
    state: tauri::State<'_, Lifecycle<tauri::Wry>>,
) -> Result<serde_json::Value, ()> {
    Ok(if state.is_exiting() {
        service::probe().await
    } else {
        service::connect().await
    })
}

/// Looks for the running service without starting it (automatic reconnection, B2.10).
#[tauri::command]
async fn service_probe() -> serde_json::Value {
    service::probe().await
}

/// `events.v1.subscribe` (B2.9): the list, its sequence and a subscription number; events follow
/// as window events until the stream ends.
#[tauri::command]
async fn events_subscribe(
    app: tauri::AppHandle,
    streams: tauri::State<'_, stream::Streams>,
) -> Result<stream::Subscribed, BridgeError> {
    stream::subscribe(app, &streams).await
}

/// Ends the window's event stream (before "Exit and stop downloads").
#[tauri::command]
fn events_unsubscribe(streams: tauri::State<'_, stream::Streams>) {
    streams.stop();
}

/// `downloads.v1.check`: existing jobs with the same link (B2.8).
#[tauri::command]
async fn downloads_check(url: String) -> Result<CheckLinkResult, BridgeError> {
    downloads::check(&data_dir()?, url).await
}

/// `downloads.v1.confirm_name` (ask-on-collision, B2.8).
#[tauri::command]
async fn downloads_confirm_name(job_id: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::confirm_name(&data_dir()?, &job_id).await
}

/// `downloads.v1.set_queue` (B2.11).
#[tauri::command]
async fn downloads_set_queue(
    job_id: String,
    queue_id: String,
) -> Result<DownloadSnapshot, BridgeError> {
    downloads::set_queue(&data_dir()?, &job_id, &queue_id).await
}

/// `downloads.v1.set_priority` (B2.11).
#[tauri::command]
async fn downloads_set_priority(
    job_id: String,
    priority: String,
) -> Result<DownloadSnapshot, BridgeError> {
    downloads::set_priority(&data_dir()?, &job_id, &priority).await
}

/// `downloads.v1.move` (B2.11).
#[tauri::command]
async fn downloads_move(job_id: String, to: String) -> Result<DownloadSnapshot, BridgeError> {
    downloads::move_job(&data_dir()?, &job_id, &to).await
}

/// `settings.v1.get` (B2.8).
#[tauri::command]
async fn service_settings_get() -> Result<ServiceSettings, BridgeError> {
    downloads::settings(&data_dir()?).await
}

/// `settings.v1.set` (B2.8): typed; folders shape-checked here, existence by the service.
#[tauri::command]
async fn service_settings_set(settings: ServiceSettings) -> Result<ServiceSettings, BridgeError> {
    downloads::set_settings(&data_dir()?, settings).await
}

/// `queues.v1.list` (B2.11).
#[tauri::command]
async fn queues_list() -> Result<ListQueuesResult, BridgeError> {
    downloads::queues(&data_dir()?).await
}

/// `queues.v1.create`.
#[tauri::command]
async fn queues_create(name: String, max_active: Option<u32>) -> Result<QueueInfo, BridgeError> {
    downloads::create_queue(&data_dir()?, &name, max_active).await
}

/// `queues.v1.rename`.
#[tauri::command]
async fn queues_rename(queue_id: String, name: String) -> Result<QueueInfo, BridgeError> {
    downloads::rename_queue(&data_dir()?, &queue_id, &name).await
}

/// `queues.v1.set_limit`.
#[tauri::command]
async fn queues_set_limit(queue_id: String, max_active: u32) -> Result<QueueInfo, BridgeError> {
    downloads::set_queue_limit(&data_dir()?, &queue_id, max_active).await
}

/// `queues.v1.delete`: never deletes jobs or files.
#[tauri::command]
async fn queues_delete(
    queue_id: String,
    move_to: Option<String>,
) -> Result<EmptyResult, BridgeError> {
    downloads::delete_queue(&data_dir()?, &queue_id, move_to.as_deref()).await
}

/// `queues.v1.start`.
#[tauri::command]
async fn queues_start(queue_id: String) -> Result<QueueInfo, BridgeError> {
    downloads::set_queue_running(&data_dir()?, &queue_id, true).await
}

/// `queues.v1.stop`.
#[tauri::command]
async fn queues_stop(queue_id: String) -> Result<QueueInfo, BridgeError> {
    downloads::set_queue_running(&data_dir()?, &queue_id, false).await
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
    category: Option<String>,
    on_duplicate: Option<String>,
    queue_id: Option<String>,
) -> Result<DownloadSnapshot, BridgeError> {
    let options = AddOptions {
        category,
        on_duplicate,
        queue_id,
    };
    downloads::add(&data_dir()?, url, idempotency_key, options).await
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
        .manage(stream::Streams::default())
        .setup(|app| {
            lifecycle::create_tray(app.handle());
            browser_setup::on_launch();
            #[cfg(feature = "smoke")]
            smoke::exit_driver(app.handle());
            #[cfg(feature = "smoke")]
            smoke::close_driver(app.handle());
            #[cfg(feature = "smoke")]
            smoke::command_driver(app.handle());
            #[cfg(feature = "smoke")]
            smoke::watch_only();
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
            service_probe,
            events_subscribe,
            events_unsubscribe,
            downloads_check,
            downloads_confirm_name,
            downloads_set_queue,
            downloads_set_priority,
            downloads_move,
            service_settings_get,
            service_settings_set,
            queues_list,
            queues_create,
            queues_rename,
            queues_set_limit,
            queues_delete,
            queues_start,
            queues_stop,
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
