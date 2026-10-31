//! B2.17 Refresh link (ADR 0020): `verify_replacement` and `discard_output` of `dm-backend-http`
//! against the controlled fixture server.
//!
//! Each case pauses a real transfer at a durable checkpoint, then asks whether a replacement link
//! serves the same content. Only a validated `206` with the same strong ETag and length whose bytes
//! equal the confirmed bytes on disk is compatible; the service decides separately whether the
//! resource is the same (origin and path) or a trusted checksum covers it. Compatible replacements
//! are then actually continued and judged by SHA-256.

#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendHandle, DownloadBackend, DurableCheckpoint, ReplacementCheck, TransferRequest,
    TransferState,
};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::AttemptId;
use dm_domain::api::ReplaceReason;
use dm_test_fixtures::{RangeMode, Route, Server, payload_seeded};
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

fn request(dir: &Path, attempt: &str, url: String) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: "out.bin".into(),
        max_connections: 1,
    }
}

fn part(dir: &Path) -> PathBuf {
    dir.join(format!("out.bin{PART_SUFFIX}"))
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn route(body: &[u8], etag: Option<&str>, slow: bool) -> Route {
    let mut r = Route::new(body.to_vec());
    r.etag = etag.map(str::to_owned);
    if slow {
        r.slow = Some(Duration::from_millis(10));
    }
    r
}

/// Starts `a.bin`, pauses it once at least 256 KiB are durable, and returns the checkpoint.
async fn paused(b: &HttpBackend, srv: &Server, dir: &Path) -> (BackendHandle, DurableCheckpoint) {
    let h = b
        .start(request(dir, "att_rs00000000001", srv.url("a.bin?sig=old")))
        .await
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(cp) = b.checkpoint(&h).await.unwrap()
            && cp.confirmed.0 >= 256 * 1024
        {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "no checkpoint");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    b.pause(&h).await.unwrap();
    let cp = b.checkpoint(&h).await.unwrap().unwrap();
    (h, cp)
}

async fn check(
    b: &HttpBackend,
    dir: &Path,
    url: String,
    cp: &DurableCheckpoint,
) -> ReplacementCheck {
    b.verify_replacement(&request(dir, "att_rs00000000002", url), cp)
        .await
        .unwrap()
}

#[tokio::test]
async fn s01_a_refreshed_signature_on_the_same_resource_keeps_progress_and_completes_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(4 * MIB, 201);
    srv.set("a.bin", route(&body, Some("\"v1\""), true));
    let b = backend();
    let (h, cp) = paused(&b, &srv, dir.path()).await;
    let new_url = srv.url("a.bin?sig=new");
    let ReplacementCheck::Compatible {
        checkpoint,
        same_resource,
    } = check(&b, dir.path(), new_url.clone(), &cp).await
    else {
        panic!("expected compatible")
    };
    assert!(same_resource, "only the query differs");
    assert_eq!(checkpoint.confirmed, cp.confirmed);
    // Nothing changed on disk by the check; continue under a new attempt from the rebound point.
    b.remove(&h).await.unwrap();
    srv.update("a.bin", |r| r.slow = None);
    let seen = srv.log_for("a.bin").len();
    let h2 = b
        .restore(
            request(dir.path(), "att_rs00000000003", new_url),
            &checkpoint,
        )
        .await
        .unwrap();
    b.resume(&h2).await.unwrap();
    let started = Instant::now();
    while b.progress(&h2).await.unwrap().state != TransferState::Complete {
        assert!(started.elapsed() < Duration::from_secs(60), "not complete");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(sha(&std::fs::read(part(dir.path())).unwrap()), sha(&body));
    let ranges: Vec<String> = srv
        .log_for("a.bin")
        .iter()
        .skip(seen)
        .filter_map(|l| l.header("range").map(str::to_owned))
        .collect();
    assert_eq!(ranges, vec![format!("bytes={}-", cp.confirmed.0)]);
}

#[tokio::test]
async fn s02_same_length_changed_content_is_never_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 202);
    srv.set("a.bin", route(&body, Some("\"v1\""), true));
    let b = backend();
    let (_h, cp) = paused(&b, &srv, dir.path()).await;
    let other = payload_seeded(2 * MIB, 999);
    // Honest server: a new ETag.
    srv.set("changed.bin", route(&other, Some("\"v2\""), false));
    assert_eq!(
        check(&b, dir.path(), srv.url("changed.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::ValidatorChanged)
    );
    // Broken server: same ETag, different bytes — caught by the byte comparison.
    srv.set("liar.bin", route(&other, Some("\"v1\""), false));
    assert_eq!(
        check(&b, dir.path(), srv.url("liar.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::ContentDiffers)
    );
    let before = std::fs::read(part(dir.path())).unwrap();
    assert_eq!(
        before.len() as u64,
        cp.confirmed.0,
        "the partial file is untouched"
    );
}

#[tokio::test]
async fn s03_missing_validators_length_changes_and_ignored_ranges_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 203);
    srv.set("a.bin", route(&body, Some("\"v1\""), true));
    let b = backend();
    let (_h, cp) = paused(&b, &srv, dir.path()).await;
    srv.set("no-etag.bin", route(&body, None, false));
    assert_eq!(
        check(&b, dir.path(), srv.url("no-etag.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::ValidatorMissing)
    );
    srv.set("weak.bin", route(&body, Some("W/\"v1\""), false));
    assert_eq!(
        check(&b, dir.path(), srv.url("weak.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::ValidatorMissing)
    );
    let mut longer = body.clone();
    longer.extend_from_slice(b"tail");
    srv.set("longer.bin", route(&longer, Some("\"v1\""), false));
    assert_eq!(
        check(&b, dir.path(), srv.url("longer.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::LengthChanged)
    );
    let mut ignoring = route(&body, Some("\"v1\""), false);
    ignoring.ranges = RangeMode::Ignore;
    srv.set("ignore.bin", ignoring);
    assert_eq!(
        check(&b, dir.path(), srv.url("ignore.bin"), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::RangeIgnored)
    );
    assert_eq!(
        check(&b, dir.path(), "ftp://example.org/a.bin".into(), &cp).await,
        ReplacementCheck::Incompatible(ReplaceReason::Unverifiable)
    );
}

#[tokio::test]
async fn s04_a_different_resource_or_origin_sharing_the_etag_is_not_the_same_resource() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 204);
    srv.set("a.bin", route(&body, Some("\"v1\""), true));
    let b = backend();
    let (_h, cp) = paused(&b, &srv, dir.path()).await;
    // Another path with the same ETag and bytes: compatible content, but not the same resource.
    srv.set("mirror.bin", route(&body, Some("\"v1\""), false));
    let ReplacementCheck::Compatible { same_resource, .. } =
        check(&b, dir.path(), srv.url("mirror.bin"), &cp).await
    else {
        panic!("expected compatible content")
    };
    assert!(!same_resource);
    // A redirect that changes the origin (127.0.0.1 → localhost) to the same path: not the same
    // resource either, whatever the ETag says.
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, srv.url_on("localhost", "a.bin?sig=elsewhere")));
    srv.set("go.bin", hop);
    let ReplacementCheck::Compatible { same_resource, .. } =
        check(&b, dir.path(), srv.url("go.bin"), &cp).await
    else {
        panic!("expected compatible content")
    };
    assert!(!same_resource, "origin changed by the redirect");
}

#[tokio::test]
async fn s05_leftover_partial_data_is_released_only_when_no_attempt_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let srv = Server::start().await;
    let body = payload_seeded(2 * MIB, 205);
    srv.set("a.bin", route(&body, Some("\"v1\""), true));
    let b = backend();
    let (h, _cp) = paused(&b, &srv, dir.path()).await;
    let req = request(dir.path(), "att_rs00000000009", srv.url("a.bin"));
    assert!(
        b.discard_output(&req).await.is_err(),
        "held by a live attempt"
    );
    assert!(part(dir.path()).exists());
    // After the attempt ended (as across a service restart), the leftover is released by name;
    // the final file name is never touched.
    b.remove(&h).await.unwrap();
    std::fs::write(dir.path().join("out.bin"), b"user file").unwrap();
    b.discard_output(&req).await.unwrap();
    assert!(!part(dir.path()).exists());
    assert_eq!(
        std::fs::read(dir.path().join("out.bin")).unwrap(),
        b"user file"
    );
    b.discard_output(&req).await.unwrap();
}
