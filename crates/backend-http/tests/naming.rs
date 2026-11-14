//! B1.6 naming inputs (ADR 0011): the backend reports the final response's `Content-Disposition`
//! and URL file name unparsed, keeps its partial file under the service-given staging name
//! whatever the server says, and classifies names the file system refuses.

#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use std::time::{Duration, Instant};

use dm_backend_api::{
    BackendHandle, DownloadBackend, ResponseNaming, TransferRequest, TransferState,
};
use dm_backend_http::{HttpBackend, HttpConfig, PART_SUFFIX};
use dm_domain::{AttemptId, ErrorKind};
use dm_test_fixtures::{Route, Server, payload};
use sha2::{Digest, Sha256};

fn backend() -> HttpBackend {
    HttpBackend::new(HttpConfig {
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn request(dir: &std::path::Path, attempt: &str, url: String, name: &str) -> TransferRequest {
    TransferRequest {
        attempt_id: AttemptId::parse(attempt).unwrap(),
        url,
        headers: Vec::new(),
        destination_dir: dir.to_owned(),
        file_name: name.into(),
        max_connections: 1,
        rate_limit: None,
        session: None,
    }
}

async fn finish(b: &HttpBackend, h: &BackendHandle) -> Option<ResponseNaming> {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if p.state == TransferState::Complete {
            return p.naming;
        }
        assert!(
            matches!(p.state, TransferState::Active | TransferState::Waiting),
            "{p:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// NB01: after a redirect, the naming inputs are the final `200`'s (its header and its URL), the
/// query never appears, and the partial file keeps the staging name.
#[tokio::test]
async fn nb01_final_response_metadata_is_reported_and_the_partial_name_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start().await;
    let body = payload(64 * 1024);
    let mut real = Route::new(body.clone());
    real.content_disposition =
        Some("attachment; filename=\"বই.pdf\"; filename*=UTF-8''%E0%A6%AC%E0%A6%87.pdf".into());
    server.set("files/final.iso", real);
    let mut hop = Route::new(Vec::new());
    hop.content_disposition = Some("attachment; filename=hop.txt".into());
    hop.redirect = Some((302, server.url("files/final.iso?sig=SECRET")));
    server.set("start", hop);

    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_00000000000000b1",
            server.url("start"),
            "start",
        ))
        .await
        .unwrap();
    let naming = finish(&b, &h).await.unwrap();
    assert_eq!(
        naming.content_disposition.as_deref(),
        Some("attachment; filename=\"বই.pdf\"; filename*=UTF-8''%E0%A6%AC%E0%A6%87.pdf"),
        "raw UTF-8 in the field arrives as text"
    );
    assert_eq!(naming.url_file_name.as_deref(), Some("final.iso"));
    let part = std::fs::read(dir.path().join(format!("start{PART_SUFFIX}"))).unwrap();
    assert_eq!(Sha256::digest(&part), Sha256::digest(&body));
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        names,
        [format!("start{PART_SUFFIX}")],
        "nothing else was created"
    );
}

/// NB02: no header means no disposition; an over-long one is dropped whole.
#[tokio::test]
async fn nb02_missing_or_oversized_headers() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start().await;
    server.set("plain", Route::new(payload(1024)));
    let mut huge = Route::new(payload(1024));
    huge.content_disposition = Some(format!("attachment; filename=\"{}.txt\"", "a".repeat(5000)));
    server.set("huge", huge);
    let b = backend();
    for (i, path) in ["plain", "huge"].into_iter().enumerate() {
        let h = b
            .start(request(
                dir.path(),
                &format!("att_00000000000000c{i}"),
                server.url(path),
                path,
            ))
            .await
            .unwrap();
        let naming = finish(&b, &h).await.unwrap();
        assert_eq!(naming.content_disposition, None, "{path}");
        assert_eq!(naming.url_file_name.as_deref(), Some(path));
    }
}

/// NB03: a name the file system refuses (300 bytes, beyond every supported limit) is reported as
/// `DESTINATION_UNWRITABLE` and creates nothing.
#[tokio::test]
async fn nb03_a_name_the_file_system_refuses_is_classified() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start().await;
    server.set("f", Route::new(payload(1024)));
    let b = backend();
    let err = b
        .start(request(
            dir.path(),
            "att_00000000000000d1",
            server.url("f"),
            &"n".repeat(300),
        ))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::DestinationUnwritable, "{err}");
    assert!(
        !err.detail.contains(&"n".repeat(50)),
        "no path in the detail"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// Waits until the transfer ends; returns its final state.
async fn settle(b: &HttpBackend, h: &BackendHandle) -> TransferState {
    let started = Instant::now();
    loop {
        let p = b.progress(h).await.unwrap();
        if matches!(p.state, TransferState::Complete | TransferState::Failed(_)) {
            return p.state;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// NB04 (Add URL matrix, 2026-11-14): a link that names a file (`.zip`, `.exe`, …) answered with an
/// HTML page, directly or after a redirect to a sign-in page, is refused as `LINK_REFRESH_REQUIRED`
/// and none of the page is written. A real page (`.html`), an address without an extension and an attachment
/// the server names keep downloading, byte for byte.
#[tokio::test]
async fn nb04_a_web_page_is_never_saved_as_the_file_the_link_names() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start().await;
    let page = b"<!doctype html><title>Sign in</title><form>login</form>".to_vec();
    let html = |body: &[u8]| {
        let mut r = Route::new(body.to_vec());
        r.content_type = Some("text/html; charset=utf-8".into());
        r
    };
    server.set("files/report.zip", html(&page));
    server.set("signin", html(&page));
    let mut moved = Route::new(Vec::new());
    moved.redirect = Some((302, server.url("signin")));
    server.set("files/setup.exe", moved);
    server.set("docs/page.html", html(&page));
    server.set("noext", html(&page));
    let mut named = html(&page);
    named.content_disposition = Some("attachment; filename=\"export.zip\"".into());
    server.set("files/export.zip", named);

    let b = backend();
    for (i, path) in ["files/report.zip", "files/setup.exe"]
        .into_iter()
        .enumerate()
    {
        let h = b
            .start(request(
                dir.path(),
                &format!("att_00000000000000e{i}"),
                server.url(path),
                &format!("refused{i}"),
            ))
            .await
            .unwrap();
        match settle(&b, &h).await {
            TransferState::Failed(e) => {
                assert_eq!(e.kind, ErrorKind::LinkRefreshRequired, "{path}: {e}");
                assert!(e.detail.contains("web page"), "{path}: {e}");
            }
            other => panic!("{path}: a page was accepted as the file: {other:?}"),
        }
    }
    for (i, path) in ["docs/page.html", "noext", "files/export.zip"]
        .into_iter()
        .enumerate()
    {
        let name = format!("kept{i}");
        let h = b
            .start(request(
                dir.path(),
                &format!("att_00000000000000f{i}"),
                server.url(path),
                &name,
            ))
            .await
            .unwrap();
        assert_eq!(settle(&b, &h).await, TransferState::Complete, "{path}");
        let got = std::fs::read(dir.path().join(format!("{name}{PART_SUFFIX}"))).unwrap();
        assert_eq!(Sha256::digest(&got), Sha256::digest(&page), "{path}");
    }
    // As for any refused first response (404, ...), at most an empty partial file remains: not one
    // byte of the page is written under the file's name.
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.starts_with("refused") {
            assert_eq!(
                entry.metadata().unwrap().len(),
                0,
                "{name} holds page bytes"
            );
        }
    }
}

/// NB05 (Add URL matrix, 2026-11-14): a redirect loop ends as `LINK_REFRESH_REQUIRED` (a way on:
/// open the link in the browser), not as an invalid request without an action.
#[tokio::test]
async fn nb05_a_redirect_loop_asks_for_a_fresh_link() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start().await;
    let mut hop = Route::new(Vec::new());
    hop.redirect = Some((302, server.url("loop")));
    server.set("loop", hop);
    let b = backend();
    let h = b
        .start(request(
            dir.path(),
            "att_00000000000000f9",
            server.url("loop"),
            "loop",
        ))
        .await
        .unwrap();
    match settle(&b, &h).await {
        TransferState::Failed(e) => {
            assert_eq!(e.kind, ErrorKind::LinkRefreshRequired, "{e}");
            assert!(e.detail.contains("kept redirecting"), "{e}");
        }
        other => panic!("a redirect loop did not fail: {other:?}"),
    }
}
