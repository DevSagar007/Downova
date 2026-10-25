//! Service readiness bridge (B0.5): find the per-user `dm-service`, or start the packaged one.
//!
//! The find-or-start logic is shared with the browser native host in
//! [`dm_platform::launch`] (reuse a compatible service, never replace an incompatible one, start
//! the sibling `dm-service` detached, bounded wait). This module only maps the result to the UI's
//! `ConnectionState` shape (apps/desktop/src/services).

use std::time::Duration;

use dm_domain::ErrorKind;
use dm_domain::api::API_VERSION;
use dm_platform::launch::{self, Readiness};
use serde_json::{Value, json};

/// How long to wait for a started service to answer.
const START_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(feature = "smoke")]
pub use dm_platform::launch::sibling_service as sidecar_path;

fn unavailable(reason: &str, detail: String) -> Value {
    let error = ErrorKind::ServiceUnavailable.to_error_data(detail);
    json!({"status": "unavailable", "reason": reason, "error": error})
}

/// Serialises concurrent `service_connect` calls inside one shell process.
static CONNECTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Finds or starts the service. Never throws: every outcome is a `ConnectionState`.
pub async fn connect() -> Value {
    let _guard = CONNECTING.lock().await;
    let Ok(data_dir) = dm_platform::paths::data_dir() else {
        return unavailable("not_running", "no per-user data directory".into());
    };
    let sidecar = launch::sibling_service();
    match launch::connect_or_start(&data_dir, sidecar.as_deref(), START_TIMEOUT).await {
        Readiness::Ready(hello) => json!({"status": "connected", "hello": hello}),
        Readiness::Incompatible(version) => unavailable(
            "incompatible_version",
            format!(
                "the running service speaks API v{version}; this app needs v{API_VERSION}. Quit the other Downova version first."
            ),
        ),
        Readiness::NotRunning(detail) => unavailable("not_running", detail),
    }
}
