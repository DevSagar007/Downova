//! ADV09 / ADR 0030 against the real `dm-service` (HTTP backend) over its IPC endpoint:
//! an update preparation holds a running transfer without changing intent, a keep-state stop and a
//! restart accept the update and resume only then (SHA-256 checked); a failed migration exits with
//! 6 and records the checked backup; `--restore-update` restores it; a corrupt backup or a lease
//! that ends without an install gives the running version back.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, PrepareUpdateResult, methods};
use dm_domain::{Blocker, DesiredState, ErrorKind, Phase};
use dm_platform::ipc::Client;
use dm_test_fixtures::{Route, Server, payload_seeded};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn service_cmd() -> Command {
    let mut c = Command::new(SERVICE);
    c.env("DOWNOVA_VAULT", "file");
    c
}

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_with(data: &Path, downloads: &Path, env: &[(&str, &str)]) -> Service {
    let mut c = service_cmd();
    c.args(["--backend", "http", "--data-dir"])
        .arg(data)
        .arg("--download-dir")
        .arg(downloads)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    Service(c.spawn().unwrap())
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
            started.elapsed() < Duration::from_secs(30),
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

async fn add(client: &mut Client, key: &str, url: &str) -> DownloadSnapshot {
    serde_json::from_value(
        client
            .call(
                methods::ADD,
                json!({"idempotency_key": key, "source": {"url": url}, "destination_profile_id": "downloads"}),
            )
            .await
            .unwrap(),
    )
    .unwrap()
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
        assert!(started.elapsed() < Duration::from_secs(90), "{what}: {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prepared_update_is_accepted_after_restart_and_held_work_resumes_only_then() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(6 * 1024 * 1024, 71);
    let mut slow = Route::new(body.clone());
    slow.etag = Some("\"upd-slow\"".into());
    slow.slow = Some(Duration::from_millis(30));
    fixture.set("slow.bin", slow);
    let paused_body = payload_seeded(1024 * 1024, 72);
    fixture.set("paused.bin", Route::new(paused_body));
    let mut service = spawn_with(data.path(), downloads.path(), &[]);
    let mut client = connect(data.path()).await;

    let running = add(&mut client, "upd-proc-run-1", &fixture.url("slow.bin")).await;
    let paused = add(&mut client, "upd-proc-pau-1", &fixture.url("paused.bin")).await;
    client
        .call(methods::PAUSE, json!({"job_id": paused.job_id}))
        .await
        .unwrap();
    wait_for(&mut client, running.job_id.as_str(), "transferring", |s| {
        s.phase == Phase::Transferring && s.downloaded_bytes.0 > 0
    })
    .await;

    // Prepare the update to this very version (the restart then counts as "installed").
    let prep: PrepareUpdateResult = serde_json::from_value(
        client
            .call(
                methods::PREPARE_UPDATE,
                json!({"operation_id": "upd-proc-op-0001", "target_version": VERSION}),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(prep.held_jobs, 1);
    let marker = data.path().join("update").join("pending.json");
    assert!(marker.exists());
    // While held: no progress, intent unchanged, new work refused.
    let held = get(&mut client, running.job_id.as_str()).await;
    assert_eq!(held.desired_state, DesiredState::Running);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let later = get(&mut client, running.job_id.as_str()).await;
    assert_eq!(
        later.downloaded_bytes, held.downloaded_bytes,
        "no transfer while held"
    );
    let refused = client
        .call(
            methods::ADD,
            json!({"idempotency_key": "upd-proc-new-1", "source": {"url": fixture.url("paused.bin")}, "destination_profile_id": "downloads"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::ServiceUnavailable);

    // The installer would run here: the service stops with keep-state, then the "new" version
    // starts, accepts the update and only then resumes the held work.
    stop(&mut client, &mut service).await;
    let mut service = spawn_with(data.path(), downloads.path(), &[]);
    let mut client = connect(data.path()).await;
    let last = read(&data.path().join("update").join("last.json"));
    assert_eq!(last["outcome"], "installed");
    assert_eq!(last["operation_id"], "upd-proc-op-0001");
    assert!(!marker.exists());
    let done = wait_for(
        &mut client,
        running.job_id.as_str(),
        "resumed and done",
        |s| s.phase.is_terminal(),
    )
    .await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    let file = std::fs::read(downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha_hex(&file), sha_hex(&body));
    assert_eq!(
        get(&mut client, paused.job_id.as_str()).await.desired_state,
        DesiredState::Paused
    );
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_migration_exits_for_recovery_and_the_checked_backup_restores() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(512 * 1024, 73);
    fixture.set("done.bin", Route::new(body.clone()));
    let mut service = spawn_with(data.path(), downloads.path(), &[]);
    let mut client = connect(data.path()).await;
    let job = add(
        &mut client,
        "upd-proc-done-1",
        &format!("{}?sig=SECRET", fixture.url("done.bin")),
    )
    .await;
    let done = wait_for(&mut client, job.job_id.as_str(), "done", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed);
    client
        .call(methods::QUEUES_STOP, json!({"queue_id": "main"}))
        .await
        .unwrap();
    client
        .call(
            methods::PREPARE_UPDATE,
            json!({"operation_id": "upd-proc-op-0002", "target_version": "99.0.0"}),
        )
        .await
        .unwrap();
    stop(&mut client, &mut service).await;

    // The next start must migrate (schema one behind) and that migration fails.
    let db = data.path().join("downova.sqlite3");
    let before = {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        conn.pragma_update(None, "user_version", v - 1).unwrap();
        v - 1
    };
    let mut failing = spawn_with(
        data.path(),
        downloads.path(),
        &[("DOWNOVA_TEST_FAIL_MIGRATION", "1")],
    );
    let code = failing.0.wait().unwrap().code();
    assert_eq!(code, Some(6), "exit code 6: recovery required");
    let recovery = read(&data.path().join("update").join("recovery.json"));
    assert_eq!(recovery["kind"], "migration_failed");
    assert_eq!(recovery["operation_id"], "upd-proc-op-0002");
    assert_eq!(recovery["backup_operation"], "upd-proc-op-0002");
    assert!(!recovery.to_string().contains("SECRET"));
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, before, "the failed migration changed nothing");
    }

    // Restore the checked backup (refused for an unknown operation), then start normally.
    let unknown = service_cmd()
        .args(["--data-dir"])
        .arg(data.path())
        .args(["--restore-update", "upd-proc-op-none"])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(unknown.code(), Some(5));
    let restored = service_cmd()
        .args(["--data-dir"])
        .arg(data.path())
        .args(["--restore-update", "upd-proc-op-0002"])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(restored.code(), Some(0));
    assert!(!data.path().join("update").join("recovery.json").exists());
    let mut service = spawn_with(data.path(), downloads.path(), &[]);
    let mut client = connect(data.path()).await;
    let back = get(&mut client, job.job_id.as_str()).await;
    assert_eq!(back.phase, Phase::Completed);
    let queues = client.call(methods::QUEUES_LIST, json!({})).await.unwrap();
    assert_eq!(
        queues["queues"][0]["running"], false,
        "the stopped queue stays stopped"
    );
    let file = std::fs::read(downloads.path().join(&back.display_name)).unwrap();
    assert_eq!(
        sha_hex(&file),
        sha_hex(&body),
        "the user's file was never touched"
    );
    assert_eq!(
        read(&data.path().join("update").join("last.json"))["outcome"],
        "restored"
    );
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupt_backup_cancels_the_preparation_and_work_continues() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(2 * 1024 * 1024, 74);
    let mut slow = Route::new(body.clone());
    slow.etag = Some("\"upd-corrupt\"".into());
    slow.slow = Some(Duration::from_millis(15));
    fixture.set("c.bin", slow);
    let mut service = spawn_with(
        data.path(),
        downloads.path(),
        &[("DOWNOVA_TEST_CORRUPT_BACKUP", "1")],
    );
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "upd-proc-cor-1", &fixture.url("c.bin")).await;
    let e = client
        .call(
            methods::PREPARE_UPDATE,
            json!({"operation_id": "upd-proc-op-0003", "target_version": "99.0.0"}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Internal);
    assert!(!data.path().join("update").join("pending.json").exists());
    // Released: the transfer finishes and new work is accepted.
    let done = wait_for(&mut client, job.job_id.as_str(), "done", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed);
    assert!(!done.blockers.contains(&Blocker::QueueStopped));
    add(&mut client, "upd-proc-cor-2", &fixture.url("c.bin")).await;
    stop(&mut client, &mut service).await;
}
