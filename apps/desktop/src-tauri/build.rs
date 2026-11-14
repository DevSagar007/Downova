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
    "update_status",
    "update_check",
    "update_download",
    "update_prepare",
    "update_cancel",
    "update_install",
    "update_restore",
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
    "concurrency_get",
    "concurrency_set",
    "speed_limit_get",
    "speed_limit_set",
    "download_speed_limit_set",
    "queues_list",
    "queues_create",
    "queues_rename",
    "queues_set_limit",
    "queues_set_schedule",
    "browser_connections",
    "browser_repair",
    "credentials_list",
    "credentials_forget",
    "diagnostics_preview",
    "diagnostics_export",
    "queues_delete",
    "queues_start",
    "queues_stop",
    "history_get",
    "history_set",
    "history_preview",
    "history_clean",
    "downloads_retry",
    "downloads_replace_source",
    "downloads_reveal",
    "folder_pick",
    "clipboard_text",
    "power_support",
    "power_run",
    "window_show",
    "notifications_get",
    "notifications_set",
];

fn main() {
    // Integration tests (tests/*.rs) link Tauri but get no application manifest from tauri-build;
    // without the Common-Controls v6 dependency they fail to load on Windows
    // (STATUS_ENTRYPOINT_NOT_FOUND). Tests only; the app binary keeps tauri-build's manifest.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-test-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
