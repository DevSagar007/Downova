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
