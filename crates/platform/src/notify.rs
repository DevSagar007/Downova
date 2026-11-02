//! System notifications for the service (CORE-09, ADR 0022), through `notify-rust`:
//! - **Windows:** a toast attributed to the installed app's AppUserModelID (the Tauri identifier).
//!   Windows shows it only for an *installed* app (a Start-menu shortcut with that ID); an
//!   extracted or development build has none, and the system then shows nothing.
//! - **macOS:** posted as the app bundle (`set_application`); the system may ask for permission
//!   the first time — that is the user's "turn on" action in Options.
//! - **Linux:** the desktop's notification service (D-Bus).
//!
//! Text only: no actions, no file paths, no URLs. A click can at most bring the app forward; it
//! never opens or runs a download.

/// Shows notifications as the installed Downova app.
///
/// Attribution differs per platform: Windows names the AppUserModelID on every toast, so the
/// notifier keeps it; macOS registers the bundle once, process-wide, in [`SystemNotifier::new`];
/// Linux has no application identity beyond the app name.
#[derive(Debug, Clone)]
pub struct SystemNotifier {
    #[cfg(windows)]
    app_id: String,
}

impl SystemNotifier {
    /// `app_id`: the desktop app's identifier (Windows AppUserModelID, macOS bundle identifier).
    pub fn new(app_id: &str) -> Self {
        #[cfg(target_os = "macos")]
        {
            // Posted as the bundle. This is process-wide and can be set only once; an error means
            // it was already set (a second notifier) or the bundle is unknown, and the default
            // sender is used. Delivery itself still reports its own failures.
            let _ = notify_rust::set_application(app_id);
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = app_id;
        Self {
            #[cfg(windows)]
            app_id: app_id.to_owned(),
        }
    }

    /// Hands one notification to the system; blocks briefly (call off the async runtime).
    pub fn show(&self, title: &str, body: &str) -> Result<(), String> {
        let mut n = notify_rust::Notification::new();
        n.summary(title).body(body).appname("Downova");
        #[cfg(windows)]
        n.app_id(&self.app_id);
        n.show().map(drop).map_err(|e| e.to_string())
    }
}
