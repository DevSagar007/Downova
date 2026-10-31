//! B2.8–B2.11 against the real `dm-service` (HTTP backend) over its private IPC endpoint:
//! destination rules, duplicates and the ask-on-collision policy with real files and SHA-256;
//! the event stream across a service restart; queue stop, priority and limits across a restart.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, EventKind, QueueInfo, methods};
use dm_domain::{Blocker, ErrorKind, Phase};
use dm_platform::ipc::{CallError, Client};
use dm_test_fixtures::{Route, Server, payload_seeded};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(data: &Path, downloads: &Path) -> Service {
    Service(
        Command::new(SERVICE)
            .args(["--backend", "http", "--data-dir"])
            .arg(data)
            .arg("--download-dir")
            .arg(downloads)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

async fn connect(data: &Path) -> Client {
    let started = Instant::now();
    loop {
        if let Ok(client) = Client::connect(data).await {
            return client;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not come up"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn stop(client: &mut Client, service: &mut Service) {
    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    let started = Instant::now();
    while service.0.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not stop"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn add(client: &mut Client, extra: Value) -> Result<DownloadSnapshot, CallError> {
    let mut params = json!({"destination_profile_id": "downloads"});
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    client
        .call(methods::ADD, params)
        .await
        .map(|v| serde_json::from_value(v).unwrap())
}

async fn get(client: &mut Client, id: &str) -> DownloadSnapshot {
    serde_json::from_value(
        client
            .call(methods::GET, json!({"job_id": id}))
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn wait_for(
    client: &mut Client,
    id: &str,
    what: &str,
    done: impl Fn(&DownloadSnapshot) -> bool,
) -> DownloadSnapshot {
    let started = Instant::now();
    loop {
        let s = get(client, id).await;
        if done(&s) {
            return s;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "{what}: {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rules_duplicates_and_ask_on_collision_with_real_files() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let videos = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let clip = payload_seeded(512 * 1024, 31);
    let named = payload_seeded(256 * 1024, 32);
    fixture.set("clip.mkv", Route::new(clip.clone()));
    fixture.set("named.bin", Route::new(named.clone()));
    let mut service = spawn(data.path(), downloads.path());
    let mut client = connect(data.path()).await;

    let settings = json!({
        "destination_rules": [{"category": "video", "directory": videos.path().display().to_string()}],
        "on_collision": "rename"
    });
    assert_eq!(
        client
            .call(methods::SETTINGS_SET, settings.clone())
            .await
            .unwrap(),
        settings
    );

    // The rule's folder receives the video, byte-exact.
    let url = format!("{}?sig=SECRET-sig", fixture.url("clip.mkv"));
    let job = add(&mut client, json!({"idempotency_key": "proc-rule-0001", "source": {"url": url}, "on_duplicate": "refuse"}))
        .await
        .unwrap();
    assert_eq!(job.category, Some(dm_domain::policy::FileCategory::Video));
    let done = wait_for(&mut client, job.job_id.as_str(), "video done", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    assert_eq!(
        sha_hex(&std::fs::read(videos.path().join("clip.mkv")).unwrap()),
        sha_hex(&clip)
    );
    assert!(!downloads.path().join("clip.mkv").exists());

    // The same link: listed by check, refused on request, added again when allowed.
    let found = client
        .call(methods::CHECK, json!({"source": {"url": url}}))
        .await
        .unwrap();
    assert_eq!(found["duplicates"][0]["job_id"], job.job_id.as_str());
    assert!(!found.to_string().contains("SECRET"));
    let refused = add(&mut client, json!({"idempotency_key": "proc-rule-0002", "source": {"url": url}, "on_duplicate": "refuse"}))
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::DuplicateDownload);
    // A replay of the first request still answers with the first job.
    let replay = add(&mut client, json!({"idempotency_key": "proc-rule-0001", "source": {"url": url}, "on_duplicate": "refuse"}))
        .await
        .unwrap();
    assert_eq!(replay.job_id, job.job_id);
    // Ask on collision: an existing file is never overwritten; the job waits for confirmation.
    client
        .call(
            methods::SETTINGS_SET,
            json!({"destination_rules": [], "on_collision": "ask"}),
        )
        .await
        .unwrap();
    std::fs::write(downloads.path().join("named.bin"), b"someone else's").unwrap();
    let held = add(
        &mut client,
        json!({"idempotency_key": "proc-ask-00001", "source": {"url": fixture.url("named.bin")}}),
    )
    .await
    .unwrap();
    let waiting = wait_for(&mut client, held.job_id.as_str(), "name held", |s| {
        s.blockers.contains(&Blocker::NameConflict)
    })
    .await;
    assert_eq!(waiting.phase, Phase::Verifying);
    assert_eq!(waiting.display_name, "named (1).bin");
    assert!(!downloads.path().join("named (1).bin").exists());
    // The hold is durable: it is still there after a restart.
    stop(&mut client, &mut service).await;
    let mut service = spawn(data.path(), downloads.path());
    let mut client = connect(data.path()).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let after = get(&mut client, held.job_id.as_str()).await;
    assert!(after.blockers.contains(&Blocker::NameConflict), "{after:?}");
    client
        .call(methods::CONFIRM_NAME, json!({"job_id": held.job_id}))
        .await
        .unwrap();
    let placed = wait_for(&mut client, held.job_id.as_str(), "placed", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(placed.phase, Phase::Completed, "{placed:?}");
    assert_eq!(
        std::fs::read(downloads.path().join("named.bin")).unwrap(),
        b"someone else's"
    );
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join("named (1).bin")).unwrap()),
        sha_hex(&named)
    );
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_event_stream_ends_with_the_service_and_restarts_with_a_new_instance() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(1024 * 1024, 33);
    fixture.set("e.bin", Route::new(body.clone()));
    let mut service = spawn(data.path(), downloads.path());
    let mut client = connect(data.path()).await;
    let mut sub = connect(data.path()).await;
    let start = sub.subscribe().await.unwrap();
    assert!(start.items.is_empty());
    let job = add(
        &mut client,
        json!({"idempotency_key": "proc-evt-00001", "source": {"url": fixture.url("e.bin")}}),
    )
    .await
    .unwrap();
    // Events arrive in sequence until the job completes (with exact byte counts).
    let mut expect = start.sequence.0;
    loop {
        let e = tokio::time::timeout(Duration::from_secs(30), sub.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        expect += 1;
        assert_eq!(e.sequence.0, expect);
        assert_eq!(e.service_instance_id, start.service_instance_id);
        if e.kind == EventKind::JobChanged
            && e.snapshot
                .as_ref()
                .is_some_and(|s| s.phase == Phase::Completed)
        {
            assert_eq!(e.snapshot.unwrap().downloaded_bytes.0, 1024 * 1024);
            break;
        }
        assert_eq!(e.job_id.as_ref(), Some(&job.job_id));
    }
    // An intentional stop ends the stream; the service is not restarted by anyone.
    stop(&mut client, &mut service).await;
    let end = tokio::time::timeout(Duration::from_secs(10), sub.next_event())
        .await
        .unwrap();
    assert!(matches!(end, Ok(None) | Err(_)), "{end:?}");
    assert!(
        Client::connect(data.path()).await.is_err(),
        "nothing restarted the service"
    );
    // A new service: a new instance, and a fresh snapshot carries the job.
    let mut service = spawn(data.path(), downloads.path());
    let mut sub = connect(data.path()).await;
    let fresh = sub.subscribe().await.unwrap();
    assert_ne!(fresh.service_instance_id, start.service_instance_id);
    assert_eq!(fresh.items.len(), 1);
    assert_eq!(fresh.items[0].phase, Phase::Completed);
    let mut client = connect(data.path()).await;
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_queue_and_its_order_survive_a_restart_and_start_finishes_the_files() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let bodies: Vec<Vec<u8>> = (0..3).map(|i| payload_seeded(768 * 1024, 40 + i)).collect();
    for (i, b) in bodies.iter().enumerate() {
        let mut r = Route::new(b.clone());
        r.etag = Some(format!("\"q{i}\""));
        r.slow = Some(Duration::from_millis(20));
        fixture.set(&format!("q{i}.bin"), r);
    }
    let mut service = spawn(data.path(), downloads.path());
    let mut client = connect(data.path()).await;
    let night: QueueInfo = serde_json::from_value(
        client
            .call(
                methods::QUEUES_CREATE,
                json!({"name": "Night", "max_active": 1}),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    let mut ids = Vec::new();
    for i in 0..3 {
        let s = add(&mut client, json!({"idempotency_key": format!("proc-queue-{i:04}"), "source": {"url": fixture.url(&format!("q{i}.bin"))}, "queue_id": night.queue_id}))
            .await
            .unwrap();
        ids.push(s.job_id.as_str().to_owned());
    }
    // One at a time in this queue; the last one moved to the top and made high priority.
    client
        .call(methods::MOVE, json!({"job_id": ids[2], "to": "top"}))
        .await
        .unwrap();
    client
        .call(
            methods::SET_PRIORITY,
            json!({"job_id": ids[2], "priority": "high"}),
        )
        .await
        .unwrap();
    client
        .call(methods::QUEUES_STOP, json!({"queue_id": night.queue_id}))
        .await
        .unwrap();
    for id in &ids {
        let s = wait_for(&mut client, id, "held", |s| {
            s.blockers.contains(&Blocker::QueueStopped)
        })
        .await;
        assert_eq!(s.desired_state, dm_domain::DesiredState::Running);
    }
    stop(&mut client, &mut service).await;

    let mut service = spawn(data.path(), downloads.path());
    let mut client = connect(data.path()).await;
    let queues: Value = client.call(methods::QUEUES_LIST, json!({})).await.unwrap();
    let saved = queues["queues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["queue_id"] == night.queue_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(saved["running"], false, "{queues}");
    assert_eq!(saved["max_active"], 1);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for id in &ids {
        let s = get(&mut client, id).await;
        assert!(s.blockers.contains(&Blocker::QueueStopped), "{s:?}");
    }
    let top = get(&mut client, &ids[2]).await;
    assert_eq!(top.priority, Some(dm_domain::policy::Priority::High));
    client
        .call(methods::QUEUES_START, json!({"queue_id": night.queue_id}))
        .await
        .unwrap();
    // Never more than one transfers at once in this queue; all finish byte-exact.
    let mut done = 0;
    let started = Instant::now();
    while done < 3 {
        let mut active = 0;
        done = 0;
        for id in &ids {
            let s = get(&mut client, id).await;
            if s.phase == Phase::Transferring {
                active += 1;
            }
            if s.phase == Phase::Completed {
                done += 1;
            }
            assert!(s.phase != Phase::Failed, "{s:?}");
        }
        assert!(active <= 1, "queue limit 1 exceeded");
        assert!(
            started.elapsed() < Duration::from_secs(90),
            "queue did not finish"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for (i, id) in ids.iter().enumerate() {
        let s = get(&mut client, id).await;
        let file = std::fs::read(downloads.path().join(&s.display_name)).unwrap();
        assert_eq!(sha_hex(&file), sha_hex(&bodies[i]), "{}", s.display_name);
    }
    stop(&mut client, &mut service).await;
}
