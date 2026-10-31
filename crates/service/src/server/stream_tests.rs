//! B2.9: the event stream over the real IPC transport.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use super::*;
use crate::core::Profiles;
use crate::fake::{FakeBackend, progress};
use dm_backend_api::{BackendHandle, TransferState};
use dm_domain::CapabilitySupport;
use dm_domain::api::{DownloadSnapshot, EventKind, methods};
use dm_platform::ipc::Client;
use serde_json::json;

async fn start() -> (
    Arc<Core<FakeBackend>>,
    String,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    dm_platform::paths::ensure_private_dir(dir.path()).unwrap();
    let mut profiles = Profiles::new();
    profiles.insert("downloads".into(), dir.path().to_owned());
    let core = Arc::new(Core::new(
        FakeBackend::with_all(CapabilitySupport::Supported),
        Some("test".into()),
        profiles,
    ));
    let bound = dm_platform::ipc::bind(dir.path()).unwrap();
    let endpoint = bound.endpoint.clone();
    let task = tokio::spawn(serve(Arc::clone(&core), bound, std::future::pending()));
    (core, endpoint, dir, task)
}

fn add(key: &str, url: &str) -> serde_json::Value {
    json!({"idempotency_key": key, "source": {"url": url}, "destination_profile_id": "downloads"})
}

/// Reads events (each exactly the next sequence) until one satisfies `done`.
async fn until(
    sub: &mut Client,
    expect: &mut u64,
    done: impl Fn(&dm_domain::api::ServiceEvent) -> bool,
) -> dm_domain::api::ServiceEvent {
    loop {
        let e = next(sub).await;
        *expect += 1;
        assert_eq!(e.sequence.0, *expect, "contiguous");
        if done(&e) {
            return e;
        }
    }
}

async fn next(sub: &mut Client) -> dm_domain::api::ServiceEvent {
    tokio::time::timeout(Duration::from_secs(5), sub.next_event())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_subscriber_gets_a_consistent_start_and_contiguous_events() {
    let (core, endpoint, _dir, task) = start().await;
    let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
    let existing: DownloadSnapshot = serde_json::from_value(
        client
            .call(
                methods::ADD,
                add("stream-key-1", "http://h/a.bin?sig=SECRET"),
            )
            .await
            .unwrap(),
    )
    .unwrap();

    let mut sub = Client::connect_endpoint(&endpoint).await.unwrap();
    let start = sub.subscribe().await.unwrap();
    assert_eq!(start.items.len(), 1);
    assert_eq!(start.items[0].job_id, existing.job_id);
    let mut expect = start.sequence.0;

    let added: DownloadSnapshot = serde_json::from_value(
        client
            .call(methods::ADD, add("stream-key-2", "http://h/b.bin"))
            .await
            .unwrap(),
    )
    .unwrap();
    let e = next(&mut sub).await;
    expect += 1;
    assert_eq!((e.sequence.0, e.kind), (expect, EventKind::JobAdded));
    assert_eq!(e.job_id.as_ref(), Some(&added.job_id));
    assert_eq!(e.service_instance_id, start.service_instance_id);

    // Progress is published by the poll loop; a pause is a semantic change.
    core.backend().set_progress(
        &BackendHandle(added.attempt_id.clone().unwrap().as_str().to_owned()),
        progress(TransferState::Active, 4096, Some(1 << 20)),
    );
    until(&mut sub, &mut expect, |e| {
        e.snapshot
            .as_ref()
            .is_some_and(|s| s.downloaded_bytes.0 == 4096)
    })
    .await;
    client
        .call(methods::PAUSE, json!({"job_id": added.job_id}))
        .await
        .unwrap();
    let e = until(&mut sub, &mut expect, |e| {
        e.snapshot
            .as_ref()
            .is_some_and(|s| s.desired_state == dm_domain::DesiredState::Paused)
    })
    .await;
    assert_eq!(e.kind, EventKind::JobChanged);
    client
        .call(methods::CANCEL, json!({"job_id": existing.job_id}))
        .await
        .unwrap();
    until(&mut sub, &mut expect, |e| {
        e.snapshot
            .as_ref()
            .is_some_and(|s| s.phase == dm_domain::Phase::Cancelled)
    })
    .await;
    client
        .call(methods::REMOVE, json!({"job_id": existing.job_id}))
        .await
        .unwrap();
    let e = until(&mut sub, &mut expect, |e| e.kind == EventKind::JobRemoved).await;
    assert_eq!(e.job_id.as_ref(), Some(&existing.job_id));
    assert!(e.snapshot.is_none());
    let all = serde_json::to_string(&e).unwrap();
    assert!(!all.contains("SECRET"));
    task.abort();
}

#[tokio::test]
async fn requests_on_a_subscribed_connection_are_answered_between_whole_lines() {
    let (_core, endpoint, _dir, task) = start().await;
    let mut sub = Client::connect_endpoint(&endpoint).await.unwrap();
    sub.subscribe().await.unwrap();
    // A second subscribe on the same connection is refused, not a second stream.
    sub.send(methods::SUBSCRIBE, json!({}), "again")
        .await
        .unwrap();
    sub.send(methods::ADD, add("stream-key-3", "http://h/c.bin"), "add-1")
        .await
        .unwrap();
    // Lines arrive whole: the refusal, then the add's answer, with its event around it.
    let mut answers = Vec::new();
    let mut events = 0;
    while answers.len() < 2 {
        let raw = tokio::time::timeout(Duration::from_secs(5), sub.raw_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        if v.get("method").is_some() {
            assert_eq!(v["method"], methods::EVENT);
            events += 1;
        } else {
            answers.push(v);
        }
    }
    assert_eq!(answers[0]["id"], "again");
    assert_eq!(answers[0]["error"]["data"]["kind"], "INVALID_REQUEST");
    assert_eq!(answers[1]["id"], "add-1");
    assert!(answers[1]["result"]["job_id"].is_string());
    assert!(events <= 1);
    task.abort();
}

#[tokio::test]
async fn a_slow_subscriber_is_cut_off_with_snapshot_required_and_never_blocks_others() {
    let (core, endpoint, _dir, task) = start().await;
    let mut client = Client::connect_endpoint(&endpoint).await.unwrap();
    let job: DownloadSnapshot = serde_json::from_value(
        client
            .call(methods::ADD, add("stream-key-4", "http://h/d.bin"))
            .await
            .unwrap(),
    )
    .unwrap();
    let handle = BackendHandle(job.attempt_id.clone().unwrap().as_str().to_owned());
    let mut slow = Client::connect_endpoint(&endpoint).await.unwrap();
    let start = slow.subscribe().await.unwrap();
    // The slow client reads nothing while thousands of progress events are published.
    for i in 1..=4_000u64 {
        core.backend()
            .set_progress(&handle, progress(TransferState::Active, i, Some(1 << 30)));
        core.poll_once().await;
    }
    // Mutations stay prompt throughout.
    let t = std::time::Instant::now();
    client
        .call(methods::PAUSE, json!({"job_id": job.job_id}))
        .await
        .unwrap();
    assert!(t.elapsed() < Duration::from_secs(2));
    // Now the slow client reads: contiguous events, then snapshot_required, then the end.
    let mut seq = start.sequence.0;
    let last = loop {
        let e = tokio::time::timeout(Duration::from_secs(10), slow.next_event())
            .await
            .unwrap()
            .unwrap();
        match e {
            Some(e) if e.kind == EventKind::SnapshotRequired => break e,
            Some(e) => {
                seq += 1;
                assert_eq!(e.sequence.0, seq, "no gap before the cut-off");
            }
            None => panic!("the stream ended without snapshot_required"),
        }
    };
    assert_eq!(last.service_instance_id, start.service_instance_id);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), slow.next_event())
            .await
            .unwrap()
            .unwrap()
            .is_none(),
        "closed after snapshot_required"
    );
    // A fresh subscription is consistent again.
    let mut again = Client::connect_endpoint(&endpoint).await.unwrap();
    let fresh = again.subscribe().await.unwrap();
    assert!(fresh.sequence.0 >= seq);
    assert_eq!(fresh.items.len(), 1);
    task.abort();
}
