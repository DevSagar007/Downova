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
    find(true).await
}

/// Finds the service without ever starting it (B2.10): the UI's automatic reconnection after a
/// restart, and every connection once "Exit and stop downloads" began, use this, so an
/// intentional stop is never undone by the window.
pub async fn probe() -> Value {
    find(false).await
}

async fn find(may_start: bool) -> Value {
    let _guard = CONNECTING.lock().await;
    let Ok(data_dir) = dm_platform::paths::data_dir() else {
        return unavailable("not_running", "no per-user data directory".into());
    };
    let sidecar = if may_start {
        launch::sibling_service()
    } else {
        None
    };
    let readiness = if may_start {
        launch::connect_or_start(&data_dir, sidecar.as_deref(), START_TIMEOUT).await
    } else {
        launch::probe(&data_dir).await
    };
    state_of(readiness, dm_domain::BUILD_VERSION)
}

/// Maps what was found to the UI's `ConnectionState`. A service of another build is never used
/// silently (REL-01, B3.6): with a matching API it could still be an older or newer installation
/// (for example one left running across an update), so the window says so instead of attaching.
fn state_of(readiness: Readiness, own_version: &str) -> Value {
    match readiness {
        Readiness::Ready(hello) if hello.service_version != own_version => unavailable(
            "incompatible_version",
            format!(
                "the running download service is Downova {}; this app is {own_version}. Quit the other Downova version first.",
                hello.service_version
            ),
        ),
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use dm_domain::api::{BackendInfo, HelloResult};

    fn hello(version: &str) -> HelloResult {
        HelloResult {
            api_version: API_VERSION,
            service_version: version.into(),
            service_instance_id: "svc_test".into(),
            backend: BackendInfo {
                id: "http".into(),
                version: None,
                capabilities: Vec::new(),
            },
        }
    }

    #[test]
    fn only_a_service_of_this_build_is_attached() {
        let same = state_of(Readiness::Ready(hello("0.1.1")), "0.1.1");
        assert_eq!(same["status"], "connected");
        // Same API, other build (an installation left running across an update): refused.
        let other = state_of(Readiness::Ready(hello("0.1.0")), "0.1.1");
        assert_eq!(other["status"], "unavailable");
        assert_eq!(other["reason"], "incompatible_version");
        let message = other["error"]["detail"].as_str().unwrap();
        assert!(
            message.contains("0.1.0") && message.contains("0.1.1"),
            "{message}"
        );
        let api = state_of(Readiness::Incompatible(API_VERSION + 1), "0.1.1");
        assert_eq!(api["reason"], "incompatible_version");
    }
}
