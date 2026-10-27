//! Window lifecycle (B2.2, UI-04, ADR 0012): close behaviour, tray, single instance, and the one
//! "Exit and stop downloads" coordinator.
//!
//! - **Close behaviour** is a desktop setting in `<data>/desktop-settings.json` (user-private).
//!   `hide_to_tray` (default) hides the window and keeps the tray; `close_window` closes the
//!   desktop UI. Downloads continue in the service either way: closing never stops it.
//! - **Tray** (official Tauri tray): a menu with "Open Downova" and "Exit and stop downloads".
//!   Reopening works through the menu, never only through icon clicks (Linux reports none). If the
//!   tray cannot be created, a hide request minimizes instead, so the window is never unreachable.
//! - **Single instance** (official plugin): a second launch focuses this window and exits.
//! - **Exit and stop**: the tray item and the application menu both ask the UI to confirm (one
//!   dialog); only a confirmed request calls [`exit_and_stop`]. It sends `service.v1.shutdown` with
//!   `pause_downloads`, then separately verifies that the service process has gone (bounded), and
//!   only then quits the desktop.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dm_domain::api::{ShutdownMode, ShutdownParams, ShutdownResult, methods};
use dm_platform::launch::{self, Readiness};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::downloads::{self, BridgeError};

/// Event the UI listens to: open the "Exit and stop downloads" confirmation.
pub const EXIT_REQUESTED: &str = "downova://exit-requested";
/// How long the service may take to exit after acknowledging (it stops transfers and the engine).
pub const STOP_TIMEOUT: Duration = Duration::from_secs(20);
const SETTINGS_FILE: &str = "desktop-settings.json";
const MAX_LABEL: usize = 80;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehaviour {
    #[default]
    HideToTray,
    CloseWindow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSettings {
    #[serde(default)]
    pub close_behaviour: CloseBehaviour,
}

/// What the UI shows in Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SettingsView {
    pub close_behaviour: CloseBehaviour,
    /// False when the tray could not be created: hiding then minimizes.
    pub tray_available: bool,
}

/// The settings file; missing or unreadable means the defaults.
pub fn load(data_dir: &Path) -> DesktopSettings {
    std::fs::read(data_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &DesktopSettings) -> std::io::Result<()> {
    dm_platform::paths::ensure_private_dir(data_dir)?;
    let bytes = serde_json::to_vec(settings).map_err(std::io::Error::other)?;
    dm_platform::paths::write_private_file(&data_dir.join(SETTINGS_FILE), &bytes)
}

/// A tray label from the UI's catalogue: non-empty, bounded, no control characters.
pub fn check_label(label: &str) -> Result<&str, BridgeError> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL || label.chars().any(char::is_control)
    {
        return Err(BridgeError::invalid_argument(
            "a tray label must be 1–80 printable characters",
        ));
    }
    Ok(label)
}

/// Shell state shared by the commands, the tray and the window events.
pub struct Lifecycle<R: Runtime> {
    data_dir: Option<PathBuf>,
    settings: Mutex<DesktopSettings>,
    tray_available: AtomicBool,
    items: Mutex<Option<(MenuItem<R>, MenuItem<R>)>>,
    exiting: AtomicBool,
}

impl<R: Runtime> std::fmt::Debug for Lifecycle<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lifecycle")
            .field("settings", &self.settings)
            .field("tray_available", &self.tray_available)
            .finish_non_exhaustive()
    }
}

impl<R: Runtime> Lifecycle<R> {
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        let settings = data_dir.as_deref().map(load).unwrap_or_default();
        Self {
            data_dir,
            settings: Mutex::new(settings),
            tray_available: AtomicBool::new(false),
            items: Mutex::new(None),
            exiting: AtomicBool::new(false),
        }
    }

    pub fn view(&self) -> SettingsView {
        SettingsView {
            close_behaviour: self.close_behaviour(),
            tray_available: self.tray_available.load(Ordering::SeqCst),
        }
    }

    pub fn close_behaviour(&self) -> CloseBehaviour {
        self.settings
            .lock()
            .map(|s| s.close_behaviour)
            .unwrap_or_default()
    }

    /// Persists first; the in-memory value changes only when the file was written.
    pub fn set_close_behaviour(
        &self,
        behaviour: CloseBehaviour,
    ) -> Result<SettingsView, BridgeError> {
        let Some(dir) = self.data_dir.as_deref() else {
            return Err(BridgeError::no_data_dir());
        };
        let next = DesktopSettings {
            close_behaviour: behaviour,
        };
        save(dir, &next).map_err(|_| BridgeError::internal("the setting could not be saved"))?;
        if let Ok(mut s) = self.settings.lock() {
            *s = next;
        }
        Ok(self.view())
    }

    pub fn set_labels(&self, open: &str, exit: &str) -> Result<(), BridgeError> {
        let (open, exit) = (check_label(open)?, check_label(exit)?);
        if let Ok(items) = self.items.lock()
            && let Some((open_item, exit_item)) = items.as_ref()
        {
            let _ = open_item.set_text(open);
            let _ = exit_item.set_text(exit);
        }
        Ok(())
    }
}

/// Shows, unminimizes and focuses the main window.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// The tray item and the application menu both end here: the UI asks for confirmation.
pub fn request_exit<R: Runtime>(app: &AppHandle<R>) {
    show_main(app);
    let _ = app.emit_to("main", EXIT_REQUESTED, ());
}

/// Creates the tray with English labels until the UI sends its catalogue's. A failure leaves
/// `tray_available` false (hiding then minimizes) and is reported, never fatal.
pub fn create_tray<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Lifecycle<R>>();
    let built = (|| -> tauri::Result<()> {
        let open = MenuItem::with_id(app, "open", "Open Downova", true, None::<&str>)?;
        let exit = MenuItem::with_id(app, "exit", "Exit and stop downloads", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&open, &exit])?;
        let mut tray = TrayIconBuilder::with_id("main")
            .tooltip("Downova")
            .menu(&menu)
            .show_menu_on_left_click(true)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open" => show_main(app),
                "exit" => request_exit(app),
                _ => {}
            });
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        tray.build(app)?;
        if let Ok(mut items) = state.items.lock() {
            *items = Some((open, exit));
        }
        Ok(())
    })();
    match built {
        Ok(()) => state.tray_available.store(true, Ordering::SeqCst),
        Err(e) => eprintln!("dm-desktop: no tray ({e}); closing will minimize instead of hiding"),
    }
}

/// What a close request did (also reported by the smoke build).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    Hidden,
    Minimized,
    Closed,
}

/// Decides a close request of the main window. Closing never touches the service.
pub fn on_close_requested<R: Runtime>(
    window: &tauri::Window<R>,
    api: &tauri::CloseRequestApi,
) -> CloseAction {
    let app = window.app_handle();
    let state = app.state::<Lifecycle<R>>();
    if state.exiting.load(Ordering::SeqCst)
        || state.close_behaviour() == CloseBehaviour::CloseWindow
    {
        return CloseAction::Closed;
    }
    api.prevent_close();
    if state.tray_available.load(Ordering::SeqCst) {
        let _ = window.hide();
        CloseAction::Hidden
    } else {
        // No tray to come back through: keep the window reachable from the taskbar or dock.
        let _ = window.minimize();
        CloseAction::Minimized
    }
}

/// Result of a confirmed "Exit and stop downloads".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExitReport {
    pub paused_jobs: u32,
    /// The service process was verified gone (its endpoint no longer answers) within the bound.
    pub stopped: bool,
}

/// Asks the service to pause every running download and stop, then verifies it stopped. Only one
/// request runs at a time; a repeated request while one is in flight is refused.
pub async fn exit_and_stop<R: Runtime>(app: &AppHandle<R>) -> Result<ExitReport, BridgeError> {
    let state = app.state::<Lifecycle<R>>();
    if state.exiting.swap(true, Ordering::SeqCst) {
        return Err(BridgeError::invalid_argument(
            "Exit and stop downloads is already in progress",
        ));
    }
    let Some(data_dir) = state.data_dir.clone() else {
        state.exiting.store(false, Ordering::SeqCst);
        return Err(BridgeError::no_data_dir());
    };
    let params = ShutdownParams {
        mode: Some(ShutdownMode::PauseDownloads),
    };
    let ack: ShutdownResult =
        match downloads::call_value(&data_dir, methods::SHUTDOWN, &params).await {
            Ok(ack) => ack,
            Err(e) => {
                // Rejected or unreachable: nothing was stopped by this request; the user may try again.
                // Uncertain: it may be stopping; the UI says so and checks the connection.
                state.exiting.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
    let stopped = wait_until_gone(&data_dir, STOP_TIMEOUT).await;
    if !stopped {
        state.exiting.store(false, Ordering::SeqCst);
    }
    Ok(ExitReport {
        paused_jobs: ack.paused_jobs,
        stopped,
    })
}

/// True once the service's endpoint stops answering within `limit`.
pub async fn wait_until_gone(data_dir: &Path, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if matches!(launch::probe(data_dir).await, Readiness::NotRunning(_)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn settings_default_persist_and_ignore_damage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).close_behaviour, CloseBehaviour::HideToTray);
        save(
            dir.path(),
            &DesktopSettings {
                close_behaviour: CloseBehaviour::CloseWindow,
            },
        )
        .unwrap();
        assert_eq!(
            load(dir.path()).close_behaviour,
            CloseBehaviour::CloseWindow
        );
        std::fs::write(
            dir.path().join(SETTINGS_FILE),
            b"{\"close_behaviour\":\"explode\"}",
        )
        .unwrap();
        assert_eq!(load(dir.path()), DesktopSettings::default());
    }

    #[test]
    fn labels_are_bounded_and_printable() {
        assert_eq!(check_label(" Open Downova ").unwrap(), "Open Downova");
        assert!(check_label("Downova খুলুন").is_ok());
        for bad in ["", "   ", "a\nb", &"x".repeat(81)] {
            assert!(check_label(bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn a_missing_service_counts_as_gone() {
        let dir = tempfile::tempdir().unwrap();
        assert!(wait_until_gone(dir.path(), Duration::from_secs(5)).await);
    }
}
