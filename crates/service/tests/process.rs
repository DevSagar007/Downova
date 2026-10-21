//! Exercises the real `dm-service` binary as a separate process (B0.2).
//!
//! Tests that need aria2c are `#[ignore]`d in plain `cargo test` and run by `pnpm verify`
//! (group `aria2`), which reports UNAVAILABLE when no aria2c binary exists.

#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{HelloResult, ListDownloadsResult, methods};
use dm_domain::capability::support_of;
use dm_domain::{Capability, CapabilitySupport, ErrorKind};
use dm_platform::ipc::Client;
use serde_json::json;

const SERVICE: &str = env!("CARGO_BIN_EXE_dm-service");

fn aria2() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "aria2c.exe"
    } else {
        "aria2c"
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("DM_ARIA2_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([root.join("tools/aria2").join(exe)])
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join(exe)),
        )
        .find(|p| p.is_file())
}

fn spawn(data: &Path, downloads: &Path, aria2: &Path) -> Child {
    Command::new(SERVICE)
        .arg("--data-dir")
        .arg(data)
        .arg("--download-dir")
        .arg(downloads)
        .arg("--aria2")
        .arg(aria2)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

async fn wait_for_endpoint(data: &Path) -> Client {
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

fn wait_exit(child: &mut Child, limit: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn missing_aria2_is_a_startup_failure() {
    let data = tempfile::tempdir().unwrap();
    let mut child = spawn(data.path(), data.path(), &data.path().join("no-aria2c"));
    assert_eq!(wait_exit(&mut child, Duration::from_secs(20)), Some(4));
}

#[test]
fn bad_arguments_exit_with_2() {
    let status = Command::new(SERVICE)
        .arg("--bogus")
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[tokio::test]
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
async fn real_service_is_single_instance_gated_and_stops_cleanly() {
    let aria2 = aria2().expect("aria2c must be available");
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = spawn(data.path(), downloads.path(), &aria2);
    let mut client = wait_for_endpoint(data.path()).await;

    // hello reports the real aria2 backend and its explicit capability report.
    let hello: HelloResult =
        serde_json::from_value(client.call(methods::HELLO, json!({})).await.unwrap()).unwrap();
    println!(
        "hello: backend {} {:?}, instance {}",
        hello.backend.id, hello.backend.version, hello.service_instance_id
    );
    assert_eq!(hello.backend.id, "aria2");
    assert_eq!(
        support_of(&hello.backend.capabilities, Capability::Http),
        CapabilitySupport::Unverified
    );
    assert_eq!(
        support_of(
            &hello.backend.capabilities,
            Capability::ResumeStrongValidator
        ),
        CapabilitySupport::Unsupported
    );

    // Second instance for the same user/data dir refuses to start.
    let mut second = spawn(data.path(), downloads.path(), &aria2);
    assert_eq!(wait_exit(&mut second, Duration::from_secs(20)), Some(3));

    // HTTP is Unverified for aria2 until ADR 0003, so add is refused — honestly, not bypassed.
    let refused = client
        .call(
            methods::ADD,
            json!({
                "idempotency_key": "process-test-1",
                "source": {"url": "http://127.0.0.1:9/data.bin?token=SYNTHETIC-SECRET"},
                "destination_profile_id": "downloads",
                "expected_size_bytes": null,
                "expected_sha256": null
            }),
        )
        .await
        .unwrap_err();
    println!("add refused: {refused}");
    assert_eq!(refused.kind(), ErrorKind::BackendCapabilityUnavailable);
    assert!(!refused.to_string().contains("SYNTHETIC-SECRET"));
    let list: ListDownloadsResult =
        serde_json::from_value(client.call(methods::LIST, json!({})).await.unwrap()).unwrap();
    assert!(list.items.is_empty());

    let unknown = client
        .call(methods::GET, json!({"job_id": "job_unknown"}))
        .await
        .unwrap_err();
    assert_eq!(unknown.kind(), ErrorKind::InvalidRequest);

    client.call(methods::SHUTDOWN, json!({})).await.unwrap();
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
    assert!(
        !dm_platform::endpoint::endpoint_file(data.path()).exists(),
        "endpoint file removed"
    );
    assert!(
        Client::connect(data.path()).await.is_err(),
        "no service after shutdown"
    );
}
