//! B0.1 controlled fixture tests against a real aria2c (docs/testing.md §1, §8).
//!
//! These need an aria2c binary, so they are `#[ignore]`d in plain `cargo test` and run by
//! `pnpm verify` (group `aria2`) as `cargo test -p dm-backend-aria2 -- --ignored`, which reports
//! UNAVAILABLE when no binary is found. The oracle is always the SHA-256 of the completed file:
//! a transfer may fail, but it must never report `Complete` with wrong bytes.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendHandle, BackendProgress, DownloadBackend, TransferRequest, TransferState,
};
use dm_backend_aria2::{Aria2Backend, LaunchConfig, binary};
use dm_domain::capability::missing_from;
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ByteCount};
use sha2::{Digest, Sha256};
use support::{Behaviour, Fixture};

const NEEDS_ARIA2: &str = "needs aria2c; run via `pnpm verify` (group aria2)";
const MIB: usize = 1024 * 1024;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Harness {
    backend: Aria2Backend,
    downloads: tempfile::TempDir,
    work: tempfile::TempDir,
}

async fn harness() -> Harness {
    let bin = binary::locate(Some(&workspace_root()))
        .await
        .expect("aria2c must be available");
    println!(
        "aria2c {} at {} (pinned: {})",
        bin.version,
        bin.path.display(),
        bin.is_pinned()
    );
    let work = tempfile::tempdir().unwrap();
    let mut config = LaunchConfig::new(bin, work.path());
    config.max_tries = 2;
    config.retry_wait_secs = 0;
    config.min_split_size = "1M".into();
    // B0.1 characterization of secret persistence needs a credential header; the production
    // default refuses them (B0.4 D02/D03).
    config.forward_credential_headers_unsafe = true;
    let backend = Aria2Backend::launch(config).await.unwrap();
    Harness {
        backend,
        downloads: tempfile::tempdir().unwrap(),
        work,
    }
}

fn request(
    h: &Harness,
    attempt: &str,
    url: String,
    name: &str,
    connections: u8,
) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: h.downloads.path().to_owned(),
        file_name: name.to_owned(),
        max_connections: connections,
    }
}

/// Polls until the transfer leaves the running states or the deadline passes.
async fn settle(
    backend: &Aria2Backend,
    handle: &BackendHandle,
    limit: Duration,
) -> BackendProgress {
    let started = Instant::now();
    loop {
        let progress = backend.progress(handle).await.unwrap();
        let running = matches!(
            progress.state,
            TransferState::Active | TransferState::Waiting
        );
        if !running || started.elapsed() > limit {
            return progress;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

fn assert_file_matches(path: &Path, fx: &Fixture) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), fx.body.len(), "length");
    assert_eq!(
        sha256(&bytes),
        sha256(&fx.body),
        "SHA-256 of completed file"
    );
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn reports_version_and_complete_capabilities() {
    let _ = NEEDS_ARIA2;
    let h = harness().await;
    assert!(!h.backend.version().is_empty());
    assert_eq!(h.backend.id(), "aria2");
    assert!(missing_from(&h.backend.capabilities()).is_empty());
    // The secret conf file must be gone once aria2 is up.
    assert!(
        std::fs::read_dir(h.work.path()).unwrap().next().is_none(),
        "work dir not empty"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn single_connection_download_matches_hash() {
    let fx = support::start(Behaviour::Ranges, 3 * MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_single", fx.url("a.bin"), "a.bin", 1))
        .await
        .unwrap();
    let done = settle(&h.backend, &handle, Duration::from_secs(30)).await;
    assert_eq!(done.state, TransferState::Complete);
    assert_eq!(done.total, Some(ByteCount(3 * MIB as u64)));
    assert_file_matches(&h.downloads.path().join("a.bin"), &fx);
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn segmented_download_uses_ranges_and_matches_hash() {
    let fx = support::start(Behaviour::Ranges, 8 * MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_seg", fx.url("s.bin"), "s.bin", 4))
        .await
        .unwrap();
    let done = settle(&h.backend, &handle, Duration::from_secs(30)).await;
    assert_eq!(done.state, TransferState::Complete);
    assert_file_matches(&h.downloads.path().join("s.bin"), &fx);
    let ranged = fx.requests().iter().filter(|r| r.range.is_some()).count();
    println!(
        "segmented: {} requests, {ranged} with Range",
        fx.requests().len()
    );
    assert!(
        ranged >= 2,
        "expected several ranged requests, saw {ranged}"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn range_ignored_never_completes_with_wrong_bytes() {
    let fx = support::start(Behaviour::IgnoreRange, 8 * MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_norange", fx.url("n.bin"), "n.bin", 4))
        .await
        .unwrap();
    let done = settle(&h.backend, &handle, Duration::from_secs(30)).await;
    println!("range ignored → {:?}", done.state);
    match done.state {
        TransferState::Complete => assert_file_matches(&h.downloads.path().join("n.bin"), &fx),
        TransferState::Failed(_) => {}
        other => panic!("unexpected final state {other:?}"),
    }
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn truncated_body_is_not_reported_complete() {
    let fx = support::start(Behaviour::Truncate, 2 * MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_trunc", fx.url("t.bin"), "t.bin", 1))
        .await
        .unwrap();
    let done = settle(&h.backend, &handle, Duration::from_secs(30)).await;
    println!("truncated → {:?}", done.state);
    assert_ne!(
        done.state,
        TransferState::Complete,
        "a truncated body must not complete"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn secret_header_is_sent_but_never_persisted() {
    const SYNTHETIC: &str = "SYNTHETIC-SECRET-b01-7f3a";
    let fx = support::start(Behaviour::Ranges, MIB).await;
    let h = harness().await;
    let mut req = request(&h, "att_secret", fx.url("p.bin"), "p.bin", 1);
    req.headers.push((
        "Authorization".into(),
        Secret::new(format!("Bearer {SYNTHETIC}")),
    ));
    let handle = h.backend.start(req).await.unwrap();
    let done = settle(&h.backend, &handle, Duration::from_secs(30)).await;
    assert_eq!(done.state, TransferState::Complete);
    assert!(
        fx.requests()
            .iter()
            .any(|r| r.authorization.as_deref() == Some(&format!("Bearer {SYNTHETIC}"))),
        "header must reach the origin"
    );
    h.backend.shutdown().await.unwrap();
    for dir in [h.work.path(), h.downloads.path()] {
        for entry in walk(dir) {
            if entry.file_name().is_some_and(|n| n == "p.bin") {
                continue; // the payload itself
            }
            let bytes = std::fs::read(&entry).unwrap();
            let found = bytes
                .windows(SYNTHETIC.len())
                .any(|w| w == SYNTHETIC.as_bytes());
            assert!(!found, "secret persisted in {}", entry.display());
        }
    }
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn pause_resume_and_remove() {
    let fx = support::start(Behaviour::Slow, 4 * MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_pause", fx.url("w.bin"), "w.bin", 1))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    h.backend.pause(&handle).await.unwrap();
    let paused = h.backend.progress(&handle).await.unwrap();
    assert_eq!(paused.state, TransferState::Paused);
    h.backend.resume(&handle).await.unwrap();
    let resumed = h.backend.progress(&handle).await.unwrap();
    assert!(matches!(
        resumed.state,
        TransferState::Active | TransferState::Waiting
    ));
    h.backend.remove(&handle).await.unwrap();
    assert!(
        h.backend.progress(&handle).await.is_err(),
        "removed attempt is unknown"
    );
    h.backend.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn handle_is_the_attempt_id_not_the_gid() {
    let fx = support::start(Behaviour::Ranges, MIB).await;
    let h = harness().await;
    let handle = h
        .backend
        .start(request(&h, "att_handle", fx.url("h.bin"), "h.bin", 1))
        .await
        .unwrap();
    assert_eq!(handle, BackendHandle("att_handle".into()));
    assert!(
        h.backend
            .start(request(&h, "att_handle", fx.url("h.bin"), "h2.bin", 1))
            .await
            .is_err()
    );
    h.backend.shutdown().await.unwrap();
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
