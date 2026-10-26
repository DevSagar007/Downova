//! B1.4: disk budget checks in `dm-backend-http` (ADR 0009). The free space is an injected value
//! (never the developer's real disk). A refusal ends the transfer as `DISK_FULL` with its last
//! durable checkpoint still exportable, so the service can continue it later.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendError, BackendHandle, DownloadBackend, SpaceGuard, TransferRequest, TransferState,
};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MIB: u64 = 1024 * 1024;

/// A budget the test controls: `needed` must not exceed `free`.
#[derive(Debug)]
struct Budget {
    free: AtomicU64,
    asked: AtomicU64,
}

impl Budget {
    fn new(free: u64) -> Arc<Self> {
        Arc::new(Self {
            free: AtomicU64::new(free),
            asked: AtomicU64::new(0),
        })
    }
}

impl SpaceGuard for Budget {
    fn ensure(&self, _dir: &Path, needed: u64) -> Result<(), BackendError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if needed > self.free.load(Ordering::SeqCst) {
            Err(BackendError::new(
                ErrorKind::DiskFull,
                "test budget exhausted",
            ))
        } else {
            Ok(())
        }
    }
}

fn backend(budget: &Arc<Budget>) -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        space: Arc::clone(budget) as Arc<dyn SpaceGuard>,
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &Path, attempt: &str, url: String, connections: u8) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: "out.bin".into(),
        max_connections: connections,
    }
}

fn part(dir: &Path) -> std::path::PathBuf {
    dir.join(format!("out.bin{PART_SUFFIX}"))
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if !matches!(p.state, TransferState::Active | TransferState::Waiting) {
            return p.state;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "did not settle: {p:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn disk_full(state: &TransferState) -> bool {
    matches!(state, TransferState::Failed(e) if e.kind == ErrorKind::DiskFull)
}

#[tokio::test]
async fn d01_a_known_size_that_does_not_fit_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    srv.set("a.bin", Route::new(payload_seeded(2 * MIB as usize, 1)));
    let budget = Budget::new(MIB);
    let b = backend(&budget);
    let h = b
        .start(request(
            dir.path(),
            "att_d010000000001",
            srv.url("a.bin"),
            1,
        ))
        .await
        .unwrap();
    assert!(disk_full(&settle(&b, &h).await));
    assert_eq!(
        std::fs::metadata(part(dir.path())).unwrap().len(),
        0,
        "no body byte written"
    );
    assert_eq!(b.checkpoint(&h).await.unwrap(), None, "nothing confirmed");
}

#[tokio::test]
async fn d02_a_segmented_file_budgets_its_whole_unconfirmed_length() {
    // 8 MiB in 4 segments: pre-sizing reserves nothing, so 6 MiB free is not enough.
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    srv.set("s.bin", Route::new(payload_seeded(8 * MIB as usize, 2)));
    let budget = Budget::new(6 * MIB);
    let b = backend(&budget);
    let h = b
        .start(request(
            dir.path(),
            "att_d020000000001",
            srv.url("s.bin"),
            4,
        ))
        .await
        .unwrap();
    assert!(disk_full(&settle(&b, &h).await));
    assert_eq!(
        std::fs::metadata(part(dir.path())).unwrap().len(),
        0,
        "the file was not even sized"
    );
}

#[tokio::test]
async fn d03_an_unknown_length_transfer_is_stopped_when_space_runs_out() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let mut r = Route::new(payload_seeded(4 * MIB as usize, 3));
    r.content_length = false;
    r.etag = None;
    r.slow = Some(Duration::from_millis(10));
    srv.set("u.bin", r);
    let budget = Budget::new(u64::MAX);
    let b = backend(&budget);
    let h = b
        .start(request(
            dir.path(),
            "att_d030000000001",
            srv.url("u.bin"),
            1,
        ))
        .await
        .unwrap();
    let started = Instant::now();
    while b.progress(&h).await.unwrap().completed.0 < 256 * 1024 {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    budget.free.store(0, Ordering::SeqCst);
    let state = settle(&b, &h).await;
    assert!(disk_full(&state), "{state:?}");
    let kept = std::fs::metadata(part(dir.path())).unwrap().len();
    assert!(kept > 0, "bytes already written are kept");
    assert!(kept < 4 * MIB, "stopped before the end");
}

#[tokio::test]
async fn d04_disk_full_keeps_the_durable_checkpoint_which_resumes_byte_exact() {
    for connections in [1u8, 4] {
        let dir = tempfile::tempdir().unwrap();
        let srv = Server::start().await;
        let body = payload_seeded(8 * MIB as usize, 4);
        let mut r = Route::new(body.clone());
        r.etag = Some("\"d4\"".into());
        // Slow enough that every segment syncs at least twice before it can finish, so the refusal
        // at the next sync always happens mid-transfer (CI and loaded machines included).
        r.slow = Some(Duration::from_millis(30));
        srv.set("k.bin", r);
        let budget = Budget::new(u64::MAX);
        let b = backend(&budget);
        let h = b
            .start(request(
                dir.path(),
                "att_d040000000001",
                srv.url("k.bin"),
                connections,
            ))
            .await
            .unwrap();
        let started = Instant::now();
        let before = loop {
            if let Some(cp) = b.checkpoint(&h).await.unwrap()
                && cp.confirmed.0 > 0
            {
                break cp;
            }
            assert!(started.elapsed() < Duration::from_secs(30));
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        // The volume fills up: the next durable sync is refused.
        budget.free.store(0, Ordering::SeqCst);
        let state = settle(&b, &h).await;
        assert!(disk_full(&state), "{connections} connection(s): {state:?}");
        let cp = b
            .checkpoint(&h)
            .await
            .unwrap()
            .expect("checkpoint still exported");
        assert!(cp.confirmed >= before.confirmed);
        b.remove(&h).await.unwrap();
        assert!(part(dir.path()).exists(), "the partial file is kept");

        // Space is back: a new attempt continues from the checkpoint.
        budget.free.store(u64::MAX, Ordering::SeqCst);
        let seen = srv.log_for("k.bin").len();
        let h2 = b
            .restore(
                request(
                    dir.path(),
                    "att_d040000000002",
                    srv.url("k.bin"),
                    connections,
                ),
                &cp,
            )
            .await
            .unwrap();
        b.resume(&h2).await.unwrap();
        assert_eq!(settle(&b, &h2).await, TransferState::Complete);
        assert_eq!(sha(&std::fs::read(part(dir.path())).unwrap()), sha(&body));
        let ranged: Vec<String> = srv
            .log_for("k.bin")
            .iter()
            .skip(seen)
            .filter_map(|l| l.header("range").map(str::to_owned))
            .collect();
        assert!(
            !ranged.is_empty(),
            "continued with ranges, not from scratch"
        );
        println!(
            "D04 {connections} connection(s): checkpoint {} B at disk full, continued with {ranged:?}",
            cp.confirmed.0
        );
    }
}
