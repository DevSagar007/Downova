// The app's own commands are listed explicitly, so a capability must grant each one (SEC-05).
const COMMANDS: &[&str] = &[
    "service_connect",
    "downloads_list",
    "downloads_add",
    "downloads_pause",
    "downloads_resume",
    "downloads_cancel",
    "downloads_remove",
    "desktop_settings_get",
    "desktop_settings_set",
    "tray_set_labels",
    "service_exit_and_stop",
    "service_probe",
    "events_subscribe",
    "events_unsubscribe",
    "downloads_check",
    "downloads_confirm_name",
    "downloads_set_queue",
    "downloads_set_priority",
    "downloads_move",
    "service_settings_get",
    "service_settings_set",
    "queues_list",
    "queues_create",
    "queues_rename",
    "queues_set_limit",
    "queues_delete",
    "queues_start",
    "queues_stop",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
