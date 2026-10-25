//! The real `dm-browser-host` executable, driven exactly as Chrome drives it: the caller origin as
//! the first argument and length-prefixed JSON on stdin/stdout (B0.6).
//!
//! Fast tests run in plain `cargo test`. The end-to-end test with a real `dm-service` builds that
//! binary and is `#[ignore]`d; `pnpm verify` runs it (group `test`, "native host end-to-end").

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects
)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use dm_domain::api::{DownloadSnapshot, NativeHostOutcome, NativeHostResponse, methods};
use dm_domain::{ErrorKind, Phase};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const ORIGIN: &str = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";
const EXE: &str = std::env::consts::EXE_SUFFIX;

fn host_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_dm-browser-host"))
}

/// A copy of the host in its own directory, so `dm-service` beside it is exactly what the test
/// puts there (nothing, or a real service).
fn isolated_host(dir: &Path, with_service: Option<&Path>) -> PathBuf {
    let host = dir.join(format!("dm-browser-host{EXE}"));
    std::fs::copy(host_exe(), &host).unwrap();
    if let Some(service) = with_service {
        std::fs::copy(service, dir.join(format!("dm-service{EXE}"))).unwrap();
    }
    host
}

fn frame(v: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(v).unwrap();
    let mut out = (body.len() as u32).to_ne_bytes().to_vec();
    out.extend(body);
    out
}

/// Runs the host with `input` on stdin; returns (exit code, decoded responses, raw stdout, stderr).
fn run(
    host: &Path,
    origin: &str,
    home: &Path,
    input: &[u8],
) -> (Option<i32>, Vec<NativeHostResponse>, Vec<u8>, String) {
    let mut child = Command::new(host)
        .arg(origin)
        .env("DOWNOVA_HOME", home)
        // A service the host starts inherits this: downloads stay inside the test directory.
        .env("DOWNOVA_DOWNLOAD_DIR", home.join("downloads"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    // stdout must be nothing but whole frames.
    let mut responses = Vec::new();
    let mut rest = out.stdout.as_slice();
    while !rest.is_empty() {
        assert!(rest.len() >= 4, "partial frame header on stdout: {rest:?}");
        let n = u32::from_ne_bytes(rest[..4].try_into().unwrap()) as usize;
        assert!(rest.len() >= 4 + n, "partial frame body on stdout");
        responses.push(serde_json::from_slice(&rest[4..4 + n]).unwrap());
        rest = &rest[4 + n..];
    }
    (
        out.status.code(),
        responses,
        out.stdout,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn add(id: &str, url: &str, key: &str) -> Value {
    json!({"v": 1, "request_id": id, "op": "add", "url": url, "idempotency_key": key})
}

#[test]
fn a_caller_not_on_the_allowlist_is_refused() {
    let home = tempfile::tempdir().unwrap();
    for origin in [
        "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
        "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf",
    ] {
        // The payload claims the right origin; only the argument Chrome supplies counts.
        let mut req = add("req_origin01", "https://h/f", "ext-00000001");
        req["origin"] = json!(ORIGIN);
        let (code, responses, _, stderr) = run(&host_exe(), origin, home.path(), &frame(&req));
        assert_eq!(code, Some(1), "{origin}");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].outcome, NativeHostOutcome::Rejected);
        assert!(stderr.contains("not on the allowlist"));
    }
}

#[test]
fn oversized_truncated_and_malformed_input_is_bounded() {
    let home = tempfile::tempdir().unwrap();
    // Oversized: refused from the prefix alone, quickly, with one framed reply.
    let started = Instant::now();
    let (code, responses, _, _) = run(&host_exe(), ORIGIN, home.path(), &u32::MAX.to_ne_bytes());
    assert_eq!(code, Some(1));
    assert_eq!(responses[0].outcome, NativeHostOutcome::Rejected);
    assert!(started.elapsed() < Duration::from_secs(5));
    // Truncated body: no partial reply on stdout.
    let mut cut = frame(&add("req_cut00001", "https://h/f", "ext-00000001"));
    cut.truncate(cut.len() - 3);
    let (code, responses, raw, _) = run(&host_exe(), ORIGIN, home.path(), &cut);
    assert_eq!(code, Some(1));
    assert!(responses.is_empty() && raw.is_empty());
    // Malformed JSON and an unsupported operation are answered, and the session continues.
    let mut input = 9u32.to_ne_bytes().to_vec();
    input.extend(b"{not json");
    input.extend(frame(&json!({"v": 1, "request_id": "req_rpc00001", "op": "rpc", "method": "service.v1.shutdown"})));
    input.extend(frame(&add("req_ftp00001", "ftp://h/f", "ext-00000001")));
    let (code, responses, _, stderr) = run(&host_exe(), ORIGIN, home.path(), &input);
    assert_eq!(code, Some(0), "clean end of input");
    assert_eq!(responses.len(), 3);
    assert!(
        responses
            .iter()
            .all(|r| r.outcome == NativeHostOutcome::Rejected)
    );
    assert_eq!(responses[1].request_id.as_deref(), Some("req_rpc00001"));
    assert!(!stderr.contains("ftp://"), "diagnostics never echo URLs");
}

#[test]
fn service_down_and_no_packaged_service_is_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let host = isolated_host(dir.path(), None);
    let input = [
        frame(&json!({"v": 1, "request_id": "req_hello001", "op": "hello"})),
        frame(&add(
            "req_down0001",
            "https://example.org/f?token=SYNTHETIC-SECRET-b06",
            "ext-00000002",
        )),
    ]
    .concat();
    let (code, responses, raw, stderr) = run(&host, ORIGIN, home.path(), &input);
    assert_eq!(code, Some(0));
    assert_eq!(responses[0].outcome, NativeHostOutcome::Unavailable);
    assert_eq!(responses[1].outcome, NativeHostOutcome::Unavailable);
    assert!(
        !String::from_utf8_lossy(&raw).contains("SYNTHETIC-SECRET")
            && !stderr.contains("SYNTHETIC-SECRET")
    );
}

/// A service stand-in that answers `hello` but never acknowledges `add` (a lost acknowledgement).
fn silent_service(home: &Path) -> std::thread::JoinHandle<()> {
    let run_dir = dm_platform::paths::run_dir(home);
    dm_platform::paths::ensure_private_dir(home).unwrap();
    dm_platform::paths::ensure_private_dir(&run_dir).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let bound = rt
        .block_on(async { dm_platform::ipc::bind(&run_dir) })
        .unwrap();
    dm_platform::endpoint::write(
        home,
        &dm_platform::endpoint::EndpointInfo {
            version: 1,
            endpoint: bound.endpoint.clone(),
            pid: std::process::id(),
        },
    )
    .unwrap();
    std::thread::spawn(move || {
        rt.block_on(async move {
            loop {
                let Ok(mut conn) = bound.accept().await else { return };
                tokio::spawn(async move {
                    while let Ok(Some(line)) = conn.read_line().await {
                        let req: Value = serde_json::from_str(&line).unwrap();
                        if req["method"] == methods::HELLO {
                            let reply = json!({"jsonrpc": "2.0", "id": req["id"], "result": {
                                "api_version": 1, "service_version": "test", "service_instance_id": "svc_silent",
                                "backend": {"id": "silent", "version": null, "capabilities": []}}});
                            let _ = conn.write_json(&reply).await;
                        } else {
                            // Swallow the add: keep the connection open, never answer.
                            tokio::time::sleep(Duration::from_secs(60)).await;
                        }
                    }
                });
            }
        });
    })
}

#[test]
fn a_lost_acknowledgement_is_uncertain_not_failed() {
    let home = tempfile::tempdir().unwrap();
    let _service = silent_service(home.path());
    let started = Instant::now();
    let (code, responses, _, _) = run(
        &host_exe(),
        ORIGIN,
        home.path(),
        &frame(&add("req_lost0001", "https://h/f", "ext-00000003")),
    );
    println!(
        "lost ack → {:?} after {:?}",
        responses[0].outcome,
        started.elapsed()
    );
    assert_eq!(code, Some(0));
    assert_eq!(responses[0].outcome, NativeHostOutcome::Uncertain);
    let e = responses[0].error.as_ref().unwrap();
    assert!(
        e.retryable
            && e.detail
                .as_deref()
                .unwrap_or_default()
                .contains("retry the same request")
    );
    assert!(started.elapsed() < Duration::from_secs(20), "bounded");
}

// ------------------------------------------------------------------ end to end (real service)

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn built_service() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "dm-service",
            "--bin",
            "dm-service",
            "--quiet",
        ])
        .current_dir(workspace())
        .status()
        .unwrap();
    assert!(status.success());
    host_exe()
        .parent()
        .unwrap()
        .join(format!("dm-service{EXE}"))
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

#[test]
#[ignore = "builds dm-service; run via `pnpm verify` (group test, native host end-to-end)"]
fn end_to_end_handoff_with_idempotency_and_service_outliving_the_host() {
    let service = built_service();
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let host = isolated_host(dir.path(), Some(&service));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(dm_test_fixtures::Server::start());
    let body = dm_test_fixtures::payload_seeded(3 * 1024 * 1024, 61);
    fixture.set("public.bin", dm_test_fixtures::Route::new(body.clone()));
    let url = fixture.url("public.bin");
    // The host finds no service, starts the packaged one beside it, and forwards the add.
    let input = [
        frame(&add("req_e2e00001", &url, "ext-e2e-key-0001")),
        // Same key + same payload (a retry after an uncertain outcome): the same job.
        frame(&add("req_e2e00002", &url, "ext-e2e-key-0001")),
        // Same key + different payload: a conflict, never a second job.
        frame(&add(
            "req_e2e00003",
            &format!("{url}?other"),
            "ext-e2e-key-0001",
        )),
    ]
    .concat();
    let (code, responses, _, stderr) = run(&host, ORIGIN, home.path(), &input);
    println!("host stderr:\n{stderr}");
    assert_eq!(code, Some(0));
    assert_eq!(
        responses[0].outcome,
        NativeHostOutcome::Accepted,
        "{:?}",
        responses[0]
    );
    let job = responses[0].job.clone().unwrap();
    assert_eq!(responses[1].outcome, NativeHostOutcome::Accepted);
    assert_eq!(
        responses[1].job.as_ref().unwrap().job_id,
        job.job_id,
        "same key → same job"
    );
    assert_eq!(responses[2].outcome, NativeHostOutcome::Rejected);
    assert_eq!(
        responses[2].error.as_ref().unwrap().kind,
        ErrorKind::IdempotencyConflict
    );

    // The host has exited; the service it started keeps running and completes the transfer.
    let done: DownloadSnapshot = rt.block_on(async {
        let started = Instant::now();
        loop {
            let mut c = dm_platform::ipc::Client::connect(home.path())
                .await
                .unwrap();
            let s: DownloadSnapshot = serde_json::from_value(
                c.call(methods::GET, json!({"job_id": job.job_id}))
                    .await
                    .unwrap(),
            )
            .unwrap();
            if s.phase.is_terminal() || started.elapsed() > Duration::from_secs(60) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let file = std::fs::read(home.path().join("downloads").join(&done.display_name)).unwrap();
    println!(
        "e2e: {:?}, {} bytes, sha256 {}",
        done.phase,
        file.len(),
        sha_hex(&file)
    );
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(sha_hex(&file), sha_hex(&body));
    let list: Value = rt.block_on(async {
        let mut c = dm_platform::ipc::Client::connect(home.path())
            .await
            .unwrap();
        c.call(methods::LIST, json!({})).await.unwrap()
    });
    assert_eq!(
        list["items"].as_array().unwrap().len(),
        1,
        "exactly one job"
    );
    // Only the URL hand-off crossed the host; no file bytes were ever on its stdout.
    rt.block_on(async {
        let mut c = dm_platform::ipc::Client::connect(home.path())
            .await
            .unwrap();
        let _ = c.call(methods::SHUTDOWN, json!({})).await;
    });
}
