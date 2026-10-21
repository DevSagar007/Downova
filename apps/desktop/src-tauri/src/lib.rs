//! Desktop shell.
//!
//! The shell presents service state and forwards user commands. It never downloads file bytes,
//! never talks to aria2, and never opens SQLite: those belong to `dm-service`
//! (see docs/architecture.md). Closing the window must not stop downloads once the service
//! exists; the shell will only *ask* the service to stop on an explicit "Exit and stop downloads".
//!
//! Foundation phase: no custom Tauri commands are registered yet. The service bridge commands
//! (`service_hello`, `service_call`, event forwarding) arrive with milestone B0/B2.

/// Builds and runs the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (for example, a missing system webview).
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri runtime");
}
