//! IPC accept loop and progress poller.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use dm_backend_api::DownloadBackend;
use dm_domain::ErrorKind;
use dm_domain::api::{RpcNotification, RpcRequest, RpcResponse, ServiceEvent, codes, methods};
use dm_platform::ipc::{BoundListener, Connection};
use tokio::sync::{mpsc, watch};

use crate::core::Core;
use crate::events::EventHub;
use crate::rpc::{After, handle_line};

/// Progress poll interval; within the 250–500 ms coalescing target (ARCH-08).
pub const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// A wall-clock gap between polls this much longer than the interval means the computer slept
/// (or the process was suspended) (B2.10).
pub const WAKE_GAP: Duration = Duration::from_secs(30);

/// True when the wall clock advanced by `gap` across one poll interval: a resume from sleep.
pub fn woke(gap: Duration) -> bool {
    gap > POLL_INTERVAL.saturating_add(WAKE_GAP)
}

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
            let mut wall = std::time::SystemTime::now();
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(POLL_INTERVAL) => {
                        let now = std::time::SystemTime::now();
                        if woke(now.duration_since(wall).unwrap_or_default()) {
                            core.on_wake().await;
                        }
                        wall = now;
                        core.poll_once().await;
                    }
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

async fn handle_connection<B: DownloadBackend + 'static>(
    core: Arc<Core<B>>,
    mut conn: Connection,
) -> After {
    loop {
        let line = match conn.read_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => return After::Continue,
        };
        if let Some(request) = subscribe_request(&line) {
            return stream(core, conn, request).await;
        }
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

/// A well-formed `events.v1.subscribe` request (anything else goes through the normal path).
fn subscribe_request(line: &str) -> Option<RpcRequest> {
    let request: RpcRequest = serde_json::from_str(line).ok()?;
    (request.jsonrpc == "2.0" && request.method == methods::SUBSCRIBE).then_some(request)
}

fn notification(event: ServiceEvent) -> RpcNotification {
    RpcNotification {
        jsonrpc: "2.0".into(),
        method: methods::EVENT.into(),
        params: event,
    }
}

/// Serves a subscribed connection (B2.9, ADR 0016). One writer sends, in order, the subscribe
/// answer, then every event and the answers to further requests on the same connection (read by
/// a separate task), so lines never interleave. The stream ends when the client goes away, when
/// a request stops the service, or when this subscriber overflowed: then it receives a final
/// `snapshot_required` and the connection closes (the client subscribes again).
async fn stream<B: DownloadBackend + 'static>(
    core: Arc<Core<B>>,
    conn: Connection,
    request: RpcRequest,
) -> After {
    let id = Some(request.id.clone());
    let (mut reader, mut writer) = conn.into_split();
    if !request
        .params
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        let error = dm_domain::api::RpcError::new(
            codes::INVALID_PARAMS,
            ErrorKind::InvalidRequest.to_error_data("invalid params: expected {}"),
        );
        let _ = writer.write_json(&RpcResponse::err(id, error)).await;
        return After::Continue;
    }
    let sub = core.subscribe().await;
    let instance = sub.start.service_instance_id.clone();
    let mut last = sub.start.sequence.0;
    let Ok(start) = serde_json::to_value(&sub.start) else {
        return After::Continue;
    };
    if writer
        .write_json(&RpcResponse::ok(id, start))
        .await
        .is_err()
    {
        return After::Continue;
    }
    let (answers_tx, mut answers) = mpsc::channel::<(RpcResponse, After)>(16);
    let reader_core = Arc::clone(&core);
    let reader_task = tokio::spawn(async move {
        while let Ok(Some(line)) = reader.read_line().await {
            let answer = handle_line(&reader_core, &line).await;
            if answers_tx.send(answer).await.is_err() {
                break;
            }
        }
    });
    let mut events = sub.events;
    let mut after = After::Continue;
    loop {
        tokio::select! {
            answer = answers.recv() => match answer {
                Some((response, then)) => {
                    if writer.write_json(&response).await.is_err() {
                        break;
                    }
                    if then == After::Shutdown {
                        after = After::Shutdown;
                        break;
                    }
                }
                // The client closed its side.
                None => break,
            },
            event = events.recv() => match event {
                Some(event) => {
                    last = event.sequence.0;
                    if writer.write_json(&notification(event)).await.is_err() {
                        break;
                    }
                }
                None => {
                    if sub.overflowed.load(std::sync::atomic::Ordering::SeqCst) {
                        let _ = writer
                            .write_json(&notification(EventHub::snapshot_required(&instance, last)))
                            .await;
                    }
                    break;
                }
            },
        }
    }
    reader_task.abort();
    after
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

#[cfg(test)]
mod stream_tests;
