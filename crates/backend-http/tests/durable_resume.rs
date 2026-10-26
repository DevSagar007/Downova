//! B1.2: durable checkpoints for `dm-backend-http` (ADR 0007, docs/reports/b1.2-crash-recovery.md).
//!
//! A "restart" here is a second `HttpBackend` instance importing a checkpoint exported by the
//! first one, after the first stopped its transfer *later* than the checkpoint, so the partial file
//! holds unconfirmed bytes past it, as after a hard kill. Real-process kill tests live in
//! `crates/service/tests/http_backend_process.rs`. Every case checks the server's request log to
//! prove where the continuation actually started, and judges the result by SHA-256.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendHandle, DownloadBackend, DurableCheckpoint, TransferRequest, TransferState,
};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::{AttemptId, ByteCount, ErrorKind};
use dm_test_fixtures::{Logged, RangeMode, Route, Server, payload_seeded};
use sha2::{Digest, Sha256};

const MIB: usize = 1024 * 1024;

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
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

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn part(dir: &Path) -> PathBuf {
    dir.join(format!("out.bin{PART_SUFFIX}"))
}

fn slow_route(body: &[u8], etag: &str, tag: &str) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = Some(etag.into());
    r.slow = Some(Duration::from_millis(10));
    r.tag = tag.into();
    r
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

/// Waits for a durable checkpoint covering at least `min` bytes while the transfer runs, lets the
/// transfer write well past it, then stops it (as a crash would: the checkpoint stays behind).
async fn checkpoint_then_crash(
    b: &HttpBackend,
    h: &BackendHandle,
    dir: &Path,
    min: u64,
) -> DurableCheckpoint {
    let started = Instant::now();
    let cp = loop {
        if let Some(cp) = b.checkpoint(h).await.unwrap()
            && cp.confirmed.0 >= min
        {
            break cp;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no durable checkpoint"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    while b.progress(h).await.unwrap().completed.0 < cp.confirmed.0 + 128 * 1024 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no progress after checkpoint"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    b.remove(h).await.unwrap();
    assert!(part(dir).exists(), "the partial file survives the crash");
    cp
}

fn ranges_after(log: &[Logged], from: usize) -> Vec<String> {
    log.iter()
        .skip(from)
        .filter_map(|l| l.header("range").map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn r01_single_connection_resumes_from_the_checkpoint_not_the_file_end() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 101);
    srv.set("a.bin", slow_route(&body, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r010000000001",
            srv.url("a.bin"),
            1,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
    let on_disk = std::fs::metadata(part(dir.path())).unwrap().len();
    assert!(
        on_disk > cp.confirmed.0,
        "unconfirmed bytes past the checkpoint"
    );
    assert_eq!(cp.backend, "http");
    assert!(cp.payload.len() <= dm_backend_api::MAX_CHECKPOINT_BYTES);

    let b2 = backend();
    let seen = srv.log_for("a.bin").len();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r010000000002", srv.url("a.bin"), 1),
            &cp,
        )
        .await
        .unwrap();
    let p = b2.progress(&h2).await.unwrap();
    assert_eq!(p.state, TransferState::Paused, "restored paused");
    assert_eq!(p.completed, cp.confirmed);
    assert_eq!(
        std::fs::metadata(part(dir.path())).unwrap().len(),
        cp.confirmed.0,
        "unconfirmed tail truncated"
    );
    b2.resume(&h2).await.unwrap();
    assert_eq!(settle(&b2, &h2).await, TransferState::Complete);
    let file = std::fs::read(dir.path().join("out.bin")).unwrap();
    assert_eq!(sha(&file), sha(&body));
    let ranges = ranges_after(&srv.log_for("a.bin"), seen);
    assert_eq!(ranges, vec![format!("bytes={}-", cp.confirmed.0)]);
    println!(
        "R01 checkpoint {} B, on disk {on_disk} B, continued with {:?}",
        cp.confirmed.0, ranges
    );
}

#[tokio::test]
async fn r02_segmented_resumes_every_segment_from_its_own_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(8 * MIB, 102);
    srv.set("s.bin", slow_route(&body, "\"s1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r020000000001",
            srv.url("s.bin"),
            4,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 512 * 1024).await;
    let b2 = backend();
    let seen = srv.log_for("s.bin").len();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r020000000002", srv.url("s.bin"), 4),
            &cp,
        )
        .await
        .unwrap();
    assert_eq!(b2.progress(&h2).await.unwrap().completed, cp.confirmed);
    b2.resume(&h2).await.unwrap();
    assert_eq!(settle(&b2, &h2).await, TransferState::Complete);
    assert_eq!(
        sha(&std::fs::read(dir.path().join("out.bin")).unwrap()),
        sha(&body)
    );
    let ranges = ranges_after(&srv.log_for("s.bin"), seen);
    let boundaries: Vec<String> = (0..4u64)
        .map(|i| format!("bytes={}-", i * 2 * MIB as u64))
        .collect();
    assert!(
        ranges
            .iter()
            .any(|r| !boundaries.iter().any(|b| r.starts_with(b))),
        "at least one segment continued from inside, not from its start: {ranges:?}"
    );
    assert!(ranges.len() <= 4, "no extra requests: {ranges:?}");
    println!(
        "R02 checkpoint {} B, continued with {ranges:?}",
        cp.confirmed.0
    );
}

#[tokio::test]
async fn r03_a_changed_etag_after_the_restart_restarts_from_zero_without_splicing() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let a = payload_seeded(4 * MIB, 103);
    let b = payload_seeded(4 * MIB, 203);
    srv.set("c.bin", slow_route(&a, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r030000000001",
            srv.url("c.bin"),
            1,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
    let mut new_route = slow_route(&b, "\"v2\"", "B");
    new_route.slow = None;
    srv.set("c.bin", new_route);
    let b2 = backend();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r030000000002", srv.url("c.bin"), 1),
            &cp,
        )
        .await
        .unwrap();
    b2.resume(&h2).await.unwrap();
    assert_eq!(settle(&b2, &h2).await, TransferState::Complete);
    let file = std::fs::read(dir.path().join("out.bin")).unwrap();
    assert_eq!(
        sha(&file),
        sha(&b),
        "the new representation, whole; never A+B"
    );
    let last = srv.log_for("c.bin").pop().unwrap();
    assert_eq!(last.header("if-range"), Some("\"v1\""));
    assert_eq!(last.served.as_deref(), Some("B"));
}

#[tokio::test]
async fn r04_segmented_with_a_changed_etag_restarts_on_one_connection() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let a = payload_seeded(8 * MIB, 104);
    let b = payload_seeded(8 * MIB, 204);
    srv.set("d.bin", slow_route(&a, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r040000000001",
            srv.url("d.bin"),
            4,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 512 * 1024).await;
    let mut new_route = slow_route(&b, "\"v2\"", "B");
    new_route.slow = None;
    srv.set("d.bin", new_route);
    let b2 = backend();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r040000000002", srv.url("d.bin"), 4),
            &cp,
        )
        .await
        .unwrap();
    b2.resume(&h2).await.unwrap();
    assert_eq!(settle(&b2, &h2).await, TransferState::Complete);
    assert_eq!(
        sha(&std::fs::read(dir.path().join("out.bin")).unwrap()),
        sha(&b)
    );
}

#[tokio::test]
async fn r05_a_server_that_stopped_honouring_ranges_restarts_from_zero() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let a = payload_seeded(4 * MIB, 105);
    srv.set("e.bin", slow_route(&a, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r050000000001",
            srv.url("e.bin"),
            1,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
    srv.update("e.bin", |r| {
        r.ranges = RangeMode::Ignore;
        r.slow = None;
    });
    let b2 = backend();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r050000000002", srv.url("e.bin"), 1),
            &cp,
        )
        .await
        .unwrap();
    b2.resume(&h2).await.unwrap();
    assert_eq!(settle(&b2, &h2).await, TransferState::Complete);
    assert_eq!(
        sha(&std::fs::read(dir.path().join("out.bin")).unwrap()),
        sha(&a),
        "a 200 is never appended: the file restarted from zero"
    );
}

#[tokio::test]
async fn r06_damaged_partial_files_are_refused_and_left_untouched() {
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 106);
    srv.set("f.bin", slow_route(&body, "\"v1\"", "A"));
    for damage in ["modified", "truncated", "deleted"] {
        let dir = tempfile::tempdir().unwrap();
        let b1 = backend();
        let h1 = b1
            .start(request(
                dir.path(),
                "att_r060000000001",
                srv.url("f.bin"),
                1,
            ))
            .await
            .unwrap();
        let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
        let p = part(dir.path());
        match damage {
            "modified" => {
                let mut bytes = std::fs::read(&p).unwrap();
                bytes[1000] ^= 0xff;
                std::fs::write(&p, &bytes).unwrap();
            }
            "truncated" => {
                let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
                f.set_len(cp.confirmed.0 - 1).unwrap();
            }
            _ => std::fs::remove_file(&p).unwrap(),
        }
        let before = std::fs::read(&p).ok();
        let b2 = backend();
        let err = b2
            .restore(
                request(dir.path(), "att_r060000000002", srv.url("f.bin"), 1),
                &cp,
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidRange, "{damage}: {err}");
        assert!(err.detail.contains(damage), "{damage}: {err}");
        assert_eq!(
            std::fs::read(&p).ok(),
            before,
            "{damage}: nothing rewritten"
        );
        assert!(!dir.path().join("out.bin").exists());
    }
}

#[tokio::test]
async fn r07_foreign_corrupt_or_newer_checkpoints_and_existing_destinations_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 107);
    srv.set("g.bin", slow_route(&body, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r070000000001",
            srv.url("g.bin"),
            1,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
    let b2 = backend();
    let mut n = 0u32;
    let mut refuse = async |c: DurableCheckpoint| {
        n += 1;
        let e = b2
            .restore(
                request(
                    dir.path(),
                    &format!("att_r07000000010{n}"),
                    srv.url("g.bin"),
                    1,
                ),
                &c,
            )
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable, "{e}");
    };
    refuse(DurableCheckpoint {
        format_version: 2,
        ..cp.clone()
    })
    .await;
    refuse(DurableCheckpoint {
        backend: "aria2".into(),
        ..cp.clone()
    })
    .await;
    refuse(DurableCheckpoint {
        payload: b"\x00garbage".to_vec(),
        ..cp.clone()
    })
    .await;
    refuse(DurableCheckpoint {
        payload: vec![b'x'; dm_backend_api::MAX_CHECKPOINT_BYTES + 1],
        ..cp.clone()
    })
    .await;
    // A file now exists at the destination: never continued over.
    std::fs::write(dir.path().join("out.bin"), b"user file").unwrap();
    let e = b2
        .restore(
            request(dir.path(), "att_r070000000201", srv.url("g.bin"), 1),
            &cp,
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidRequest);
    assert_eq!(
        std::fs::read(dir.path().join("out.bin")).unwrap(),
        b"user file"
    );
}

#[tokio::test]
async fn r08_a_restored_attempt_sends_nothing_until_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 108);
    srv.set("h.bin", slow_route(&body, "\"v1\"", "A"));
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r080000000001",
            srv.url("h.bin"),
            1,
        ))
        .await
        .unwrap();
    let cp = checkpoint_then_crash(&b1, &h1, dir.path(), 256 * 1024).await;
    let b2 = backend();
    let seen = srv.log_for("h.bin").len();
    let h2 = b2
        .restore(
            request(dir.path(), "att_r080000000002", srv.url("h.bin"), 1),
            &cp,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(srv.log_for("h.bin").len(), seen, "no request while paused");
    assert_eq!(b2.progress(&h2).await.unwrap().state, TransferState::Paused);
    // Its own checkpoint can be exported again (the service rebinds it to the new attempt).
    let again = b2.checkpoint(&h2).await.unwrap().unwrap();
    assert_eq!(again.confirmed, cp.confirmed);
    assert_eq!(again.confirmed, ByteCount(cp.confirmed.0));
}

#[tokio::test]
async fn r09_transfers_without_a_strong_validator_never_export_a_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 109);
    let mut r = Route::new(body);
    r.etag = None;
    r.slow = Some(Duration::from_millis(10));
    srv.set("w.bin", r);
    let b1 = backend();
    let h1 = b1
        .start(request(
            dir.path(),
            "att_r090000000001",
            srv.url("w.bin"),
            1,
        ))
        .await
        .unwrap();
    let started = Instant::now();
    while b1.progress(&h1).await.unwrap().completed.0 < (MIB as u64) {
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(b1.checkpoint(&h1).await.unwrap(), None);
    b1.remove(&h1).await.unwrap();
}
