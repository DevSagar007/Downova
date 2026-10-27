//! IPC accept loop and progress poller.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use dm_backend_api::DownloadBackend;
use dm_platform::ipc::{BoundListener, Connection};
use tokio::sync::watch;

use crate::core::Core;
use crate::rpc::{After, handle_line};

/// Progress poll interval; within the 250–500 ms coalescing target (ARCH-08).
pub const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// Serves until `shutdown` resolves or a client calls `service.v1.shutdown`.
pub async fn serve<B, F>(core: Arc<Core<B>>, bound: BoundListener, shutdown: F)
where
    B: DownloadBackend + 'static,
    F: Future<Output = ()>,
{
    let (stop_tx, mut stop_rx) = watch::channel(false);

    let poller = {
        let core = Arc::clone(&core);
        let mut stop = stop_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(POLL_INTERVAL) => core.poll_once().await,
                    _ = stop.changed() => break,
                }
            }
        })
    };

    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = bound.accept() => {
                let Ok(conn) = accepted else { continue };
                let core = Arc::clone(&core);
                let stop_tx = stop_tx.clone();
                tokio::spawn(async move {
                    if handle_connection(core, conn).await == After::Shutdown {
                        let _ = stop_tx.send(true);
                    }
                });
            }
            () = &mut shutdown => break,
            _ = stop_rx.changed() => break,
        }
    }
    let _ = stop_tx.send(true);
    let _ = poller.await;
}

async fn handle_connection<B: DownloadBackend>(core: Arc<Core<B>>, mut conn: Connection) -> After {
    loop {
        let line = match conn.read_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => return After::Continue,
        };
        let (response, after) = handle_line(&core, &line).await;
        if conn.write_json(&response).await.is_err() {
            // Client went away before reading the acknowledgement; the operation itself already
            // happened and is replay-safe through the idempotency key.
            return after;
        }
        if after == After::Shutdown {
            return after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Profiles;
    use crate::fake::FakeBackend;
    use dm_domain::api::{DownloadSnapshot, ListDownloadsResult, methods};
    use dm_domain::{CapabilitySupport, ErrorKind};
    use dm_platform::ipc::{CallError, Client};
    use serde_json::json;

    async fn start(fake: FakeBackend) -> (String, tempfile::TempDir, tokio::task::JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        dm_platform::paths::ensure_private_dir(dir.path()).unwrap();
        let mut profiles = Profiles::new();
        profiles.insert("downloads".into(), dir.path().to_owned());
        let core = Arc::new(Core::new(fake, Some("test".into()), profiles));
        let bound = dm_platform::ipc::bind(dir.path()).unwrap();
        let endpoint = bound.endpoint.clone();
        let task = tokio::spawn(serve(core, bound, std::future::pending()));
        (endpoint, dir, task)
    }

    fn add(key: &str) -> serde_json::Value {
        json!({
            "idempotency_key": key,
            "source": {"url": "http://fixture.invalid/data.bin?token=SYNTHETIC-SECRET"},
            "destination_profile_id": "downloads",
            "expected_size_bytes": null,
            "expected_sha256": null
        })
    }

    #[tokio::test]
    async fn round_trip_over_ipc_with_lost_acknowledgement_retry() {
        let (endpoint, _dir, task) =
            start(FakeBackend::with_all(CapabilitySupport::Supported)).await;

        // Client 1 sends add and disappears without reading the acknowledgement.
        let mut lost = Client::connect_endpoint(&endpoint).await.unwrap();
        lost.send(methods::ADD, add("lost-ack-key-1"), "1")
            .await
            .unwrap();
        drop(lost);
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Client 2 retries with the same key: same job, no duplicate.
        let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
        let snap: DownloadSnapshot = serde_json::from_value(
            client
                .call(methods::ADD, add("lost-ack-key-1"))
                .await
                .unwrap(),
        )
        .unwrap();
        let list: ListDownloadsResult =
            serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].job_id, snap.job_id);
        assert!(
            !serde_json::to_string(&list)
                .unwrap()
                .contains("SYNTHETIC-SECRET")
        );

        let get = client
            .call(methods::GET, json!({"job_id": snap.job_id}))
            .await
            .unwrap();
        assert_eq!(get["job_id"], snap.job_id.as_str());
        let paused = client
            .call(methods::PAUSE, json!({"job_id": snap.job_id}))
            .await
            .unwrap();
        assert_eq!(paused["desired_state"], "paused");
        let resumed = client
            .call(methods::RESUME, json!({"job_id": snap.job_id}))
            .await
            .unwrap();
        assert_eq!(resumed["desired_state"], "running");
        let cancelled = client
            .call(methods::CANCEL, json!({"job_id": snap.job_id}))
            .await
            .unwrap();
        assert_eq!(cancelled["phase"], "cancelled");

        client.call(methods::SHUTDOWN, json!({})).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn protocol_and_domain_errors_are_structured() {
        let (endpoint, _dir, task) =
            start(FakeBackend::with_all(CapabilitySupport::Unverified)).await;
        let mut client = Client::connect_endpoint(&endpoint).await.unwrap();

        let gated = client
            .call(methods::ADD, add("gated-key-01"))
            .await
            .unwrap_err();
        assert_eq!(gated.kind(), ErrorKind::BackendCapabilityUnavailable);

        let unknown = client
            .call(methods::GET, json!({"job_id": "job_nope"}))
            .await
            .unwrap_err();
        assert_eq!(unknown.kind(), ErrorKind::InvalidRequest);

        let bad_params = client
            .call(methods::GET, json!({"job": 1}))
            .await
            .unwrap_err();
        assert!(matches!(&bad_params, CallError::Rpc(e) if e.code == -32602));

        let unknown_method = client.call("aria2.addUri", json!({})).await.unwrap_err();
        assert!(matches!(&unknown_method, CallError::Rpc(e) if e.code == -32601));

        task.abort();
    }

    /// ADR 0012: a rejected or malformed shutdown request never stops the service; an accepted one
    /// is acknowledged with its mode and then stops it.
    #[tokio::test]
    async fn only_a_valid_shutdown_request_stops_the_service() {
        let (endpoint, _dir, task) =
            start(FakeBackend::with_all(CapabilitySupport::Supported)).await;
        let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
        let job: DownloadSnapshot = serde_json::from_value(
            client
                .call(methods::ADD, add("exit-key-0001"))
                .await
                .unwrap(),
        )
        .unwrap();
        for bad in [
            json!({"mode": "kill_everything"}),
            json!({"mode": "pause_downloads", "force": true}),
            json!({"mode": 1}),
            json!([]),
        ] {
            let refused = client
                .call(methods::SHUTDOWN, bad.clone())
                .await
                .unwrap_err();
            assert!(
                matches!(&refused, CallError::Rpc(e) if e.code == -32602),
                "{bad}: {refused:?}"
            );
        }
        // Still serving, and nothing was paused by the rejected requests.
        let still: DownloadSnapshot = serde_json::from_value(
            client
                .call(methods::GET, json!({"job_id": job.job_id}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(still.desired_state, dm_domain::DesiredState::Running);
        assert!(!task.is_finished());

        let ack = client
            .call(methods::SHUTDOWN, json!({"mode": "pause_downloads"}))
            .await
            .unwrap();
        assert_eq!(ack, json!({"mode": "pause_downloads", "paused_jobs": 1}));
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
    }
}
