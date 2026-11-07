//! BRW-05 reconnect end to end (ADR 0028): the real `dm-service` (debug: file vault, the HTTPS
//! fixture's test CA through `DOWNOVA_TEST_CA_DER`), the real HTTP engine and a real HTTPS server.
//!
//! - R1 a temporary sign-in lost with a hard kill: the job is held with its checkpoint; a
//!   reconnect continues it with a range request carrying only the fresh cookie; SHA-256 exact.
//! - R2 the site rejects the session (401) after a pause: held once (no retry storm), the
//!   partial kept; "Reconnect and resume" completes it from where it was.
//! - R3 the resource changed while the job waited: the old partial is never spliced with the new
//!   representation (the engine's strong-validator rule restarts it cleanly).
//! - Every file in the data folder is free of both cookies afterwards.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, methods};
use dm_domain::{Blocker, Phase};
use dm_platform::ipc::Client;
use dm_test_fixtures::{Route, Scripted, Server, payload, tls};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");
const MIB: usize = 1024 * 1024;
const OLD: &str = "SYNTH-OLD-SESSION-b33r";
const FRESH: &str = "SYNTH-FRESH-SESSION-b33r";
const PROFILE: &str = "0123456789abcdef";

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Env {
    data: tempfile::TempDir,
    downloads: tempfile::TempDir,
    trace: tempfile::TempDir,
    ca_file: tempfile::NamedTempFile,
    srv: Server,
}

async fn env() -> Env {
    let ca = tls::TestCa::new("Downova Test CA");
    let ca_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(ca_file.path(), ca.der().as_ref()).unwrap();
    Env {
        data: tempfile::tempdir().unwrap(),
        downloads: tempfile::tempdir().unwrap(),
        trace: tempfile::tempdir().unwrap(),
        ca_file,
        srv: Server::start_tls(ca.leaf(&["localhost", "127.0.0.1"])).await,
    }
}

impl Env {
    fn spawn(&self) -> Service {
        Service(
            Command::new(SERVICE)
                .env("DOWNOVA_VAULT", "file")
                .env("DOWNOVA_TEST_CA_DER", self.ca_file.path())
                .env(
                    "DOWNOVA_CHECKPOINT_TRACE",
                    self.trace.path().join("t.jsonl"),
                )
                .args(["--backend", "http", "--data-dir"])
                .arg(self.data.path())
                .arg("--download-dir")
                .arg(self.downloads.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    async fn client(&self) -> Client {
        let started = Instant::now();
        loop {
            if let Ok(c) = Client::connect(self.data.path()).await {
                return c;
            }
            assert!(started.elapsed() < Duration::from_secs(20), "no service");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_checkpoint(&self, job: &str) -> u64 {
        let started = Instant::now();
        loop {
            let text =
                std::fs::read_to_string(self.trace.path().join("t.jsonl")).unwrap_or_default();
            if let Some(c) = text
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|e| e["event"] == "checkpoint" && e["job_id"] == job)
                .filter_map(|e| e["confirmed"].as_u64())
                .find(|c| *c > 0)
            {
                return c;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "no checkpoint");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn origin(&self) -> String {
        self.srv
            .url_on("127.0.0.1", "")
            .trim_end_matches('/')
            .to_owned()
    }

    fn assert_no_cookie_on_disk(&self) {
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(self.data.path(), &mut files);
        for p in files {
            let bytes = std::fs::read(&p).unwrap_or_default();
            for secret in [OLD, FRESH] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "{} holds {secret}",
                    p.display()
                );
            }
        }
    }
}

fn session(origin: &str, value: &str) -> Value {
    json!({
        "origin": origin, "browser": "chrome", "profile_id": PROFILE, "remember": false,
        "cookies": [{"name": "sid", "value": value, "domain": "127.0.0.1", "host_only": true,
                     "path": "/", "secure": true}]
    })
}

async fn call(c: &mut Client, method: &str, params: Value) -> DownloadSnapshot {
    serde_json::from_value(c.call(method, params).await.unwrap()).unwrap()
}

async fn add(e: &Env, c: &mut Client, key: &str, path: &str) -> DownloadSnapshot {
    let url = format!("{}/{path}", e.origin());
    call(
        c,
        methods::ADD_AUTHENTICATED,
        json!({
            "add": {"idempotency_key": key, "source": {"url": url},
                    "destination_profile_id": "downloads"},
            "session": session(&e.origin(), OLD),
        }),
    )
    .await
}

async fn reconnect(
    e: &Env,
    c: &mut Client,
    job: &DownloadSnapshot,
    op: &str,
    resume: bool,
) -> DownloadSnapshot {
    call(
        c,
        methods::RECONNECT,
        json!({
            "job_id": job.job_id, "operation_id": op,
            "expected_credential_id": job.auth.as_ref().unwrap().credential_id,
            "session": session(&e.origin(), FRESH), "resume": resume,
        }),
    )
    .await
}

async fn get(c: &mut Client, id: &str) -> DownloadSnapshot {
    call(c, methods::GET, json!({"job_id": id})).await
}

async fn wait_for(
    c: &mut Client,
    id: &str,
    what: &str,
    f: impl Fn(&DownloadSnapshot) -> bool,
) -> DownloadSnapshot {
    let started = Instant::now();
    loop {
        let s = get(c, id).await;
        if f(&s) {
            return s;
        }
        assert!(started.elapsed() < Duration::from_secs(90), "{what}: {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn slow(body: &[u8], etag: &str) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some(etag.into());
    r.slow = Some(Duration::from_millis(40));
    r
}

fn held(s: &DownloadSnapshot) -> bool {
    s.blockers.contains(&Blocker::SignInRequired)
}

#[tokio::test(flavor = "multi_thread")]
async fn r1_a_temporary_sign_in_lost_with_a_kill_is_reconnected_and_resumes_its_partial_data() {
    let e = env().await;
    let body = payload(3 * MIB / 2);
    e.srv.set("r1.bin", slow(&body, "\"r1\""));
    let mut svc = e.spawn();
    let mut c = e.client().await;
    let job = add(&e, &mut c, "ext-r1-key-0001", "r1.bin").await;
    let confirmed = e.wait_checkpoint(job.job_id.as_str()).await;
    svc.0.kill().unwrap();
    svc.0.wait().unwrap();
    drop(c);
    let before = e.srv.log_for("r1.bin").len();
    // Fast now, so the rest completes quickly.
    e.srv.set("r1.bin", {
        let mut r = Route::new(body.clone());
        r.etag = Some("\"r1\"".into());
        r
    });
    let _svc = e.spawn();
    let mut c = e.client().await;
    let held_job = wait_for(&mut c, job.job_id.as_str(), "held", held).await;
    assert_eq!(
        held_job.auth.as_ref().unwrap().state,
        dm_domain::api::CredentialState::Missing
    );
    assert_eq!(
        e.srv.log_for("r1.bin").len(),
        before,
        "nothing was requested without the session"
    );
    let r = reconnect(&e, &mut c, &held_job, "ext-r1-reconnect-01", false).await;
    assert_eq!(r.job_id, job.job_id, "the same job");
    let done = wait_for(&mut c, job.job_id.as_str(), "completed", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(e.downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha(&file), sha(&body));
    let after: Vec<_> = e.srv.log_for("r1.bin").into_iter().skip(before).collect();
    assert!(!after.is_empty());
    for l in &after {
        assert_eq!(
            l.header("cookie"),
            Some(format!("sid={FRESH}").as_str()),
            "only the fresh sign-in"
        );
    }
    let first = after[0].header("range").unwrap_or_default().to_owned();
    assert!(
        first.starts_with(&format!("bytes={confirmed}-")) || first.starts_with("bytes="),
        "resumed from its partial data, not from zero: {first:?}"
    );
    assert_ne!(first, "bytes=0-", "never from zero");
    e.assert_no_cookie_on_disk();
    println!(
        "r1: resumed with {first}, {} requests after the reconnect",
        after.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn r2_a_rejected_session_is_held_once_and_reconnect_and_resume_completes() {
    let e = env().await;
    let body = payload(3 * MIB / 2);
    e.srv.set("r2.bin", slow(&body, "\"r2\""));
    let _svc = e.spawn();
    let mut c = e.client().await;
    let job = add(&e, &mut c, "ext-r2-key-0001", "r2.bin").await;
    e.wait_checkpoint(job.job_id.as_str()).await;
    call(&mut c, methods::PAUSE, json!({"job_id": job.job_id})).await;
    // The site ends the session: the next request is refused.
    e.srv.update("r2.bin", |r| {
        r.script.push_back(Scripted::status(401, "Unauthorized"));
        r.slow = None;
    });
    let before = e.srv.log_for("r2.bin").len();
    call(&mut c, methods::RESUME, json!({"job_id": job.job_id})).await;
    let h = wait_for(&mut c, job.job_id.as_str(), "held", held).await;
    assert_eq!(
        h.auth.as_ref().unwrap().state,
        dm_domain::api::CredentialState::Expired
    );
    assert_ne!(h.phase, Phase::Failed);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        e.srv.log_for("r2.bin").len(),
        before + 1,
        "one refused request, no retries"
    );
    call(&mut c, methods::PAUSE, json!({"job_id": job.job_id})).await;
    let r = reconnect(&e, &mut c, &h, "ext-r2-reconnect-01", true).await;
    assert_eq!(r.desired_state, dm_domain::DesiredState::Running);
    let done = wait_for(&mut c, job.job_id.as_str(), "completed", |s| {
        s.phase.is_terminal()
    })
    .await;
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    let file = std::fs::read(e.downloads.path().join(&done.display_name)).unwrap();
    assert_eq!(sha(&file), sha(&body));
    let after: Vec<_> = e
        .srv
        .log_for("r2.bin")
        .into_iter()
        .skip(before + 1)
        .collect();
    assert!(
        after
            .iter()
            .all(|l| l.header("cookie") == Some(format!("sid={FRESH}").as_str()))
    );
    assert!(
        after
            .iter()
            .any(|l| l.header("range").is_some_and(|r| r != "bytes=0-"))
    );
    e.assert_no_cookie_on_disk();
}

#[tokio::test(flavor = "multi_thread")]
async fn r3_a_resource_changed_meanwhile_is_never_spliced_with_the_old_partial_data() {
    let e = env().await;
    let a = payload(3 * MIB / 2);
    let b: Vec<u8> = a.iter().map(|x| x ^ 0x5a).collect();
    e.srv.set("r3.bin", slow(&a, "\"r3a\""));
    let mut svc = e.spawn();
    let mut c = e.client().await;
    let job = add(&e, &mut c, "ext-r3-key-0001", "r3.bin").await;
    e.wait_checkpoint(job.job_id.as_str()).await;
    svc.0.kill().unwrap();
    svc.0.wait().unwrap();
    drop(c);
    let mut changed = Route::new(b.clone());
    changed.etag = Some("\"r3b\"".into());
    e.srv.set("r3.bin", changed);
    let _svc = e.spawn();
    let mut c = e.client().await;
    let h = wait_for(&mut c, job.job_id.as_str(), "held", held).await;
    reconnect(&e, &mut c, &h, "ext-r3-reconnect-01", false).await;
    let done = wait_for(&mut c, job.job_id.as_str(), "finished", |s| {
        s.phase.is_terminal()
    })
    .await;
    let mixed = done.phase == Phase::Completed && {
        let file = std::fs::read(e.downloads.path().join(&done.display_name)).unwrap();
        sha(&file) != sha(&b) && sha(&file) != sha(&a)
    };
    assert!(!mixed, "old and new bytes were spliced");
    if done.phase == Phase::Completed {
        let file = std::fs::read(e.downloads.path().join(&done.display_name)).unwrap();
        assert_eq!(sha(&file), sha(&b), "the whole new representation");
    } else {
        assert_eq!(
            done.error.as_ref().map(|x| x.kind),
            Some(dm_domain::ErrorKind::RemoteChanged),
            "{done:?}"
        );
    }
    println!("r3: {:?} {:?}", done.phase, done.error.map(|x| x.kind));
    e.assert_no_cookie_on_disk();
}
