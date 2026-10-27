// The app's own commands are listed explicitly, so a capability must grant each one (SEC-05).
const COMMANDS: &[&str] = &[
    "service_connect",
    "downloads_list",
    "downloads_add",
    "downloads_pause",
    "downloads_resume",
    "downloads_cancel",
    "desktop_settings_get",
    "desktop_settings_set",
    "tray_set_labels",
    "service_exit_and_stop",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
