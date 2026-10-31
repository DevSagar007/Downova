//! B2.16/B2.17 (ADR 0020) against the real `dm-service` (HTTP backend) over its private IPC
//! endpoint: Retry of a failed job, Refresh link keeping or restarting progress, repeated
//! requests, and hard kills at the release/restore boundaries. Every completed file is judged by
//! SHA-256, and the server's request log proves where continuations started.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, ReplaceSourceResult, methods};
use dm_domain::{DesiredState, ErrorKind, NextAction, Phase};
use dm_platform::ipc::{CallError, Client};
use dm_test_fixtures::{Route, Scripted, Server, payload_seeded};
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

fn spawn(data: &Path, downloads: &Path, fault: Option<&str>) -> Service {
    let mut cmd = Command::new(SERVICE);
    cmd.args(["--backend", "http", "--data-dir"])
        .arg(data)
        .arg("--download-dir")
        .arg(downloads)
        .env_remove("DOWNOVA_FAULT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(f) = fault {
        cmd.env("DOWNOVA_FAULT", f);
    }
    Service(cmd.spawn().unwrap())
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

async fn exited(service: &mut Service) {
    let started = Instant::now();
    while service.0.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not exit"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn stop(client: &mut Client, service: &mut Service) {
    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    exited(service).await;
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
        assert!(started.elapsed() < Duration::from_secs(60), "{what}: {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn retry(client: &mut Client, id: &str, op: &str, discard: bool) -> Result<Value, CallError> {
    client
        .call(
            methods::RETRY,
            json!({"job_id": id, "operation_id": op, "discard_partial": discard}),
        )
        .await
}

async fn replace(
    client: &mut Client,
    id: &str,
    op: &str,
    url: &str,
    mode: &str,
) -> Result<ReplaceSourceResult, CallError> {
    client
        .call(
            methods::REPLACE_SOURCE,
            json!({"job_id": id, "operation_id": op, "url": url, "mode": mode}),
        )
        .await
        .map(|v| serde_json::from_value(v).unwrap())
}

fn slow(body: &[u8], etag: &str) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some(etag.into());
    r.slow = Some(Duration::from_millis(10));
    r
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_restarts_a_failed_job_once_also_across_a_kill_before_it_was_recorded() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(1024 * 1024, 41);
    // The first request fails with 412 (REMOTE_CHANGED: never retried automatically).
    let mut route = slow(&body, "\"r1\"");
    route
        .script
        .push_back(Scripted::status(412, "Precondition Failed"));
    fixture.set("retry.bin", route);
    // Killed right after the old partial is released, before the reopen is recorded.
    let mut service = spawn(data.path(), downloads.path(), Some("after_retry_release"));
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "proc-retry-0001", &fixture.url("retry.bin")).await;
    let failed = wait_for(&mut client, job.job_id.as_str(), "failed", |s| {
        s.phase == Phase::Failed
    })
    .await;
    assert_eq!(failed.error.unwrap().kind, ErrorKind::RemoteChanged);
    assert!(
        retry(&mut client, job.job_id.as_str(), "op_proc_rt_01", true)
            .await
            .is_err()
    );
    exited(&mut service).await;
    // After the kill: still failed (nothing recorded); the same operation now applies once.
    let mut service = spawn(data.path(), downloads.path(), None);
    let mut client = connect(data.path()).await;
    assert_eq!(
        get(&mut client, job.job_id.as_str()).await.phase,
        Phase::Failed
    );
    let first = retry(&mut client, job.job_id.as_str(), "op_proc_rt_01", true)
        .await
        .unwrap();
    // A lost acknowledgement: the client repeats; the answer is the same attempt.
    let again = retry(&mut client, job.job_id.as_str(), "op_proc_rt_01", true)
        .await
        .unwrap();
    assert_eq!(first["job_id"], again["job_id"]);
    assert_eq!(first["attempt_id"], again["attempt_id"]);
    let done = wait_for(&mut client, job.job_id.as_str(), "done", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join("retry.bin")).unwrap()),
        sha_hex(&body)
    );
    // Completed: never retried again; one job for the link.
    let refused = retry(&mut client, job.job_id.as_str(), "op_proc_rt_02", true)
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::InvalidRequest);
    let list = client.call(methods::LIST, json!({})).await.unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_link_keeps_proven_progress_across_a_kill_and_completes_byte_exact() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(4 * 1024 * 1024, 42);
    fixture.set("signed.bin", slow(&body, "\"s1\""));
    let mut service = spawn(data.path(), downloads.path(), Some("after_replace_restore"));
    let mut client = connect(data.path()).await;
    let old_url = format!("{}?sig=SECRET-old", fixture.url("signed.bin"));
    let new_url = format!("{}?sig=SECRET-new", fixture.url("signed.bin"));
    let job = add(&mut client, "proc-refresh-01", &old_url).await;
    let id = job.job_id.as_str().to_owned();
    wait_for(&mut client, &id, "progress", |s| {
        s.downloaded_bytes.0 >= 512 * 1024
    })
    .await;
    client
        .call(methods::PAUSE, json!({"job_id": id}))
        .await
        .unwrap();
    // Killed after the replacement was restored, before it was recorded.
    assert!(
        replace(
            &mut client,
            &id,
            "op_proc_rs_01",
            &new_url,
            "keep_if_verified"
        )
        .await
        .is_err()
    );
    exited(&mut service).await;
    let mut service = spawn(data.path(), downloads.path(), None);
    let mut client = connect(data.path()).await;
    let recovered = wait_for(&mut client, &id, "recovered", |s| s.attempt_id.is_some()).await;
    assert_eq!(recovered.desired_state, DesiredState::Paused);
    let kept = recovered.downloaded_bytes;
    assert!(
        kept.0 >= 512 * 1024,
        "the old checkpoint survived: {recovered:?}"
    );
    // The same operation applies now, exactly once.
    let r = replace(
        &mut client,
        &id,
        "op_proc_rs_01",
        &new_url,
        "keep_if_verified",
    )
    .await
    .unwrap();
    assert_eq!(
        r.outcome,
        dm_domain::api::ReplaceOutcome::ProgressKept,
        "{r:?}"
    );
    assert_eq!(r.job.downloaded_bytes, kept);
    let again = replace(
        &mut client,
        &id,
        "op_proc_rs_01",
        &new_url,
        "keep_if_verified",
    )
    .await
    .unwrap();
    assert_eq!(again.job.attempt_id, r.job.attempt_id);
    assert!(!serde_json::to_string(&r).unwrap().contains("SECRET"));
    fixture.update("signed.bin", |route| route.slow = None);
    let seen = fixture.log_for("signed.bin").len();
    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    let done = wait_for(&mut client, &id, "done", |s| s.phase.is_terminal()).await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join("signed.bin")).unwrap()),
        sha_hex(&body)
    );
    // Continued, never restarted: every request after the refresh is a validated range request
    // (one per segment that still had bytes to fetch).
    let log = fixture.log_for("signed.bin");
    let after: Vec<_> = log.iter().skip(seen).collect();
    assert!(!after.is_empty());
    assert!(
        after
            .iter()
            .all(|l| l.header("range").is_some_and(|r| r != "bytes=0-")),
        "no full restart"
    );
    stop(&mut client, &mut service).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_content_needs_an_explicit_restart_and_an_expired_link_is_refreshed() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let fixture = Server::start().await;
    let body = payload_seeded(4 * 1024 * 1024, 43);
    let changed = payload_seeded(4 * 1024 * 1024, 44);
    fixture.set("orig.bin", slow(&body, "\"o1\""));
    let mut other = Route::new(changed.clone());
    other.etag = Some("\"o1\"".into());
    fixture.set("other.bin", other);
    let mut service = spawn(data.path(), downloads.path(), None);
    let mut client = connect(data.path()).await;
    let job = add(&mut client, "proc-change-01", &fixture.url("orig.bin")).await;
    let id = job.job_id.as_str().to_owned();
    wait_for(&mut client, &id, "progress", |s| {
        s.downloaded_bytes.0 >= 256 * 1024
    })
    .await;
    client
        .call(methods::PAUSE, json!({"job_id": id}))
        .await
        .unwrap();
    let before = get(&mut client, &id).await;
    // Same length, same ETag, different bytes on another path: never spliced.
    let r = replace(
        &mut client,
        &id,
        "op_proc_ch_01",
        &fixture.url("other.bin"),
        "keep_if_verified",
    )
    .await
    .unwrap();
    assert_eq!(r.outcome, dm_domain::api::ReplaceOutcome::ChoiceRequired);
    assert_eq!(
        r.job.downloaded_bytes, before.downloaded_bytes,
        "Keep current: unchanged"
    );
    assert_eq!(r.job.attempt_id, before.attempt_id);
    // Restart from zero with the replacement (explicit), then resume.
    let r = replace(
        &mut client,
        &id,
        "op_proc_ch_02",
        &fixture.url("other.bin"),
        "restart",
    )
    .await
    .unwrap();
    assert_eq!(r.outcome, dm_domain::api::ReplaceOutcome::Restarted);
    client
        .call(methods::RESUME, json!({"job_id": id}))
        .await
        .unwrap();
    let done = wait_for(&mut client, &id, "done", |s| s.phase.is_terminal()).await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join("orig.bin")).unwrap()),
        sha_hex(&changed),
        "the replacement's bytes only, never a splice"
    );

    // An expired link (403) fails; Retry points to Refresh link; a refresh restarts it.
    let mut expired = Route::new(body.clone());
    expired.script.push_back(Scripted::status(403, "Forbidden"));
    fixture.set("exp.bin", expired);
    let job = add(&mut client, "proc-expire-01", &fixture.url("exp.bin")).await;
    let id = job.job_id.as_str().to_owned();
    let failed = wait_for(&mut client, &id, "failed", |s| s.phase == Phase::Failed).await;
    assert_eq!(failed.error.unwrap().kind, ErrorKind::LinkRefreshRequired);
    let refused = retry(&mut client, &id, "op_proc_ex_01", true)
        .await
        .unwrap_err();
    let CallError::Rpc(e) = refused else {
        panic!("expected a refusal")
    };
    assert_eq!(e.data.unwrap().next_action, NextAction::RefreshLink);
    let r = replace(
        &mut client,
        &id,
        "op_proc_ex_02",
        &format!("{}?fresh=1", fixture.url("exp.bin")),
        "keep_if_verified",
    )
    .await
    .unwrap();
    assert_eq!(r.reason, Some(dm_domain::api::ReplaceReason::NoCheckpoint));
    replace(
        &mut client,
        &id,
        "op_proc_ex_03",
        &format!("{}?fresh=1", fixture.url("exp.bin")),
        "restart",
    )
    .await
    .unwrap();
    let done = wait_for(&mut client, &id, "done", |s| s.phase.is_terminal()).await;
    assert_eq!(done.phase, Phase::Completed, "{done:?}");
    assert_eq!(
        sha_hex(&std::fs::read(downloads.path().join("exp.bin")).unwrap()),
        sha_hex(&body)
    );
    stop(&mut client, &mut service).await;
}
