//! ADV09 / ADR 0030: the real `tauri-plugin-updater` verifier against disposable keys.
//!
//! Run through `node scripts/update/verifier-fixture.mjs`, which signs the cases with the pinned
//! Tauri CLI and passes them in `DOWNOVA_UPDATE_FIXTURE` (ignored otherwise). Each case is served
//! over loopback HTTP and goes through the plugin's own check, download and verification, with
//! the configuration Downova compiles in (`requireSignedVersion: true`, an exact package target).
//! Insecure transport is allowed here only because the endpoint is the test's own loopback server.

#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tauri_plugin_updater::{Error, UpdaterExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TARGET: &str = "windows-x86_64-nsis";

#[derive(Clone)]
enum Body {
    Full(Vec<u8>),
    /// Announces the full length, sends half, then closes.
    Cut(Vec<u8>),
}

/// A tiny loopback HTTP server: `/latest.json` answers the manifest, `/artifact` the body.
async fn serve(manifest: Arc<Mutex<Value>>, body: Arc<Mutex<Body>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let manifest = Arc::clone(&manifest);
            let body = Arc::clone(&body);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_owned();
                if path.starts_with("/latest.json") {
                    let text = manifest.lock().unwrap().to_string();
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        text.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(text.as_bytes()).await;
                } else {
                    let b = body.lock().unwrap().clone();
                    let (bytes, cut) = match &b {
                        Body::Full(x) => (x.clone(), false),
                        Body::Cut(x) => (x.clone(), true),
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let send = if cut {
                        &bytes[..bytes.len() / 2]
                    } else {
                        &bytes[..]
                    };
                    let _ = sock.write_all(send).await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    base
}

fn manifest(version: &str, signature: &str, base: &str, platform: &str) -> Value {
    json!({
        "version": version,
        "notes": "fixture",
        "pub_date": "2026-11-07T00:00:00Z",
        "platforms": { platform: { "signature": signature, "url": format!("{base}/artifact") } }
    })
}

fn app(pubkey: &str, endpoint: &str, insecure: bool) -> tauri::App<tauri::test::MockRuntime> {
    try_app(pubkey, endpoint, insecure).unwrap()
}

fn try_app(
    pubkey: &str,
    endpoint: &str,
    insecure: bool,
) -> tauri::Result<tauri::App<tauri::test::MockRuntime>> {
    let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
    ctx.config_mut().version = Some("0.1.0".into());
    ctx.package_info_mut().version = "0.1.0".parse().unwrap();
    let mut cfg = json!({
        "pubkey": pubkey,
        "endpoints": [endpoint],
        "requireSignedVersion": true,
    });
    if insecure {
        cfg["dangerousInsecureTransportProtocol"] = json!(true);
    }
    ctx.config_mut().plugins.0.insert("updater".into(), cfg);
    tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(ctx)
}

/// `Ok(Some(bytes))` verified, `Ok(None)` no update, `Err` the plugin's refusal.
async fn attempt(pubkey: &str, base: &str, insecure: bool) -> Result<Option<Vec<u8>>, Error> {
    let app = app(pubkey, &format!("{base}/latest.json"), insecure);
    let updater = app.handle().updater_builder().target(TARGET).build()?;
    let Some(update) = updater.check().await? else {
        return Ok(None);
    };
    assert_eq!(update.version, update.version.trim());
    update.download(|_, _| {}, || {}).await.map(Some)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs signed fixtures: node scripts/update/verifier-fixture.mjs"]
async fn the_plugin_verifies_signature_and_signed_version_against_the_compiled_key() {
    let path = std::env::var("DOWNOVA_UPDATE_FIXTURE").expect("DOWNOVA_UPDATE_FIXTURE");
    let cases: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let pubkey = cases["pubkey"].as_str().unwrap();
    let artifact = std::fs::read(cases["artifact"].as_str().unwrap()).unwrap();
    let manifest_slot = Arc::new(Mutex::new(Value::Null));
    let body = Arc::new(Mutex::new(Body::Full(artifact.clone())));
    let base = serve(Arc::clone(&manifest_slot), Arc::clone(&body)).await;
    let set = |m: Value, b: Body| {
        *manifest_slot.lock().unwrap() = m;
        *body.lock().unwrap() = b;
    };
    let sig = |k: &str| cases[k].as_str().unwrap().to_owned();

    // 1. Valid: signed by the trusted key, bound to the announced version.
    set(
        manifest("0.2.0", &sig("valid"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    let bytes = attempt(pubkey, &base, true).await.unwrap().unwrap();
    assert_eq!(bytes, artifact);
    println!("valid signed update: verified");

    // 2. Tampered artifact: one byte changed after signing.
    let mut tampered = artifact.clone();
    tampered[1000] ^= 0x01;
    set(
        manifest("0.2.0", &sig("valid"), &base, TARGET),
        Body::Full(tampered),
    );
    let e = attempt(pubkey, &base, true).await.unwrap_err();
    assert!(matches!(e, Error::Minisign(_)), "tampered: {e}");

    // 3. Signed with another key.
    set(
        manifest("0.2.0", &sig("wrong_key"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    let e = attempt(pubkey, &base, true).await.unwrap_err();
    assert!(matches!(e, Error::Minisign(_)), "wrong key: {e}");

    // 4. Missing signature (empty): refused before anything is accepted.
    set(
        manifest("0.2.0", "", &base, TARGET),
        Body::Full(artifact.clone()),
    );
    assert!(
        attempt(pubkey, &base, true).await.is_err(),
        "missing signature"
    );

    // 5. Valid signature without a signed version: refused (requireSignedVersion is enforced).
    set(
        manifest("0.2.0", &sig("unbound"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    let e = attempt(pubkey, &base, true).await.unwrap_err();
    assert!(matches!(e, Error::MissingSignedVersion), "unbound: {e}");

    // 6. Signed for 0.1.9 but announced as 0.2.0: refused.
    set(
        manifest("0.2.0", &sig("other_version"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    let e = attempt(pubkey, &base, true).await.unwrap_err();
    assert!(
        matches!(e, Error::SignedVersionMismatch { .. }),
        "mismatch: {e}"
    );

    // 7. An older (or equal) version is not an update: nothing is downloaded.
    set(
        manifest("0.0.9", &sig("valid"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    assert!(attempt(pubkey, &base, true).await.unwrap().is_none());
    set(
        manifest("0.1.0", &sig("valid"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    assert!(attempt(pubkey, &base, true).await.unwrap().is_none());

    // 8. Only another package format is offered: not selected.
    set(
        manifest("0.2.0", &sig("valid"), &base, "linux-x86_64-deb"),
        Body::Full(artifact.clone()),
    );
    let e = attempt(pubkey, &base, true).await.unwrap_err();
    assert!(matches!(e, Error::TargetNotFound(_)), "wrong target: {e}");

    // 9. Interrupted download: never verified, never returned.
    set(
        manifest("0.2.0", &sig("valid"), &base, TARGET),
        Body::Cut(artifact.clone()),
    );
    assert!(
        attempt(pubkey, &base, true).await.is_err(),
        "interrupted download"
    );

    // 10. Plain HTTP without the fixture-only transport flag: the plugin refuses it in release
    // builds and only warns in debug builds. Downova does not rely on that: its own gate
    // (`update::check_config`, unit-tested) refuses an HTTP configuration in every non-fixture
    // binary, so such an updater is never registered.
    set(
        manifest("0.2.0", &sig("valid"), &base, TARGET),
        Body::Full(artifact.clone()),
    );
    if !cfg!(debug_assertions) {
        // Release: the plugin refuses the configuration itself, before any request is made.
        let Err(e) = try_app(pubkey, &format!("{base}/latest.json"), false) else {
            panic!("a plain-HTTP updater must not initialise in a release build");
        };
        assert!(
            e.to_string().contains("secure protocol"),
            "insecure transport: {e}"
        );
        println!("release build: plain HTTP refused at plugin initialisation");
        println!("all refusal cases refused");
        return;
    }
    let plain = attempt(pubkey, &base, false).await;
    if cfg!(debug_assertions) {
        println!(
            "debug build: the plugin only warns about plain HTTP; the Downova gate refuses it"
        );
    } else {
        let e = plain.unwrap_err();
        assert!(
            matches!(e, Error::InsecureTransportProtocol),
            "insecure transport: {e}"
        );
    }
    println!("all refusal cases refused");
}
