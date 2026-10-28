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

/// A copy of the host in its own directory, with an executable named `dm-service` beside it.
fn isolated_host(dir: &Path, beside: Option<&Path>) -> PathBuf {
    let host = dir.join(format!("dm-browser-host{EXE}"));
    std::fs::copy(host_exe(), &host).unwrap();
    if let Some(service) = beside {
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

type Run = (Option<i32>, Vec<NativeHostResponse>, Vec<u8>, String);

/// Runs the host with `input` on stdin; returns (exit code, decoded responses, raw stdout, stderr).
/// Every write error is a test failure: a host that is expected to read its input must read it.
fn run(host: &Path, origin: &str, home: &Path, input: &[u8]) -> Run {
    run_with(host, origin, home, input, false)
}

/// Like [`run`], for a host that is expected to answer and exit *before* reading stdin (a refused
/// caller). Whether our write lands before or after that exit is a race, so only a closed pipe
/// (`BrokenPipe`; ERROR_NO_DATA on Windows maps to it) is tolerated; any other write error fails.
fn run_refused(host: &Path, origin: &str, home: &Path, input: &[u8]) -> Run {
    run_with(host, origin, home, input, true)
}

fn run_with(host: &Path, origin: &str, home: &Path, input: &[u8], may_close_stdin: bool) -> Run {
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
    let written = child.stdin.take().unwrap().write_all(input);
    match written {
        Ok(()) => {}
        Err(e) if may_close_stdin && e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("writing the request to the host failed: {e}");
        }
    }
    // Bounded: every host run here ends on its own (end of input or refusal); a hang is killed.
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the host did not exit within 60 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
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
        let mut req = add(
            "req_origin01",
            "https://h/f.zip?token=SYNTHETIC-SECRET-origin",
            "ext-00000001",
        );
        req["origin"] = json!(ORIGIN);
        // The host refuses from the argument alone and exits without reading stdin.
        let (code, responses, raw, stderr) =
            run_refused(&host_exe(), origin, home.path(), &frame(&req));
        assert_eq!(code, Some(1), "{origin}");
        assert_eq!(responses.len(), 1, "exactly one complete framed reply");
        let refusal = &responses[0];
        assert_eq!(refusal.outcome, NativeHostOutcome::Rejected);
        assert_eq!(refusal.request_id, None, "the request was never read");
        assert!(refusal.job.is_none(), "no job");
        assert_eq!(
            refusal.error.as_ref().map(|e| e.kind),
            Some(ErrorKind::InvalidRequest)
        );
        assert!(stderr.contains("not on the allowlist"), "{stderr}");
        for out in [stderr.as_str(), &String::from_utf8_lossy(&raw)] {
            assert!(!out.contains("SYNTHETIC-SECRET"), "{out}");
        }
    }
}

/// The order CI #19 hit on Linux, made deterministic: the refused host has already exited when the
/// request is written. The write may fail only with a closed pipe, and the one framed refusal is on
/// stdout.
#[test]
fn a_refused_caller_is_answered_before_its_input_is_read() {
    use std::io::Read;
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(host_exe())
        .arg("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/")
        .env("DOWNOVA_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("a refused host must exit on its own");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.code(), Some(1));
    // The host has exited, so nothing will ever read this. Usually the write fails with a closed
    // pipe; on Linux it can also succeed, because a child that another test is spawning at this
    // moment may still hold a copy of the read end until its exec closes it (seen on CI #21,
    // ubuntu-24.04). Either is fine; any other error is not.
    let written = stdin
        .write_all(&frame(&add("req_late0001", "https://h/f", "ext-00000001")))
        .and_then(|()| stdin.flush());
    if let Err(e) = written {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{e}");
    }
    let mut raw = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut raw).unwrap();
    let n = u32::from_ne_bytes(raw[..4].try_into().unwrap()) as usize;
    assert_eq!(raw.len(), 4 + n, "exactly one complete frame");
    let reply: NativeHostResponse = serde_json::from_slice(&raw[4..]).unwrap();
    assert_eq!(reply.outcome, NativeHostOutcome::Rejected);
    assert!(reply.job.is_none());
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
fn service_down_is_unavailable_and_the_host_never_starts_one() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // A dm-service sits right beside the host: it must still not be started (see NOT_RUNNING).
    let host = isolated_host(dir.path(), Some(&host_exe()));
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
        responses[1]
            .error
            .as_ref()
            .unwrap()
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("open the Downova app"),
        "actionable"
    );
    assert!(
        !dm_platform::endpoint::endpoint_file(home.path()).exists(),
        "no service was started by the host"
    );
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

/// A service started by the test; dropping it (success or panic) kills and reaps it if it is still
/// running, so a failed assertion cannot leave a service holding `target/debug/dm-service` open.
struct Service(std::process::Child);

impl std::ops::Deref for Service {
    type Target = std::process::Child;
    fn deref(&self) -> &std::process::Child {
        &self.0
    }
}

impl std::ops::DerefMut for Service {
    fn deref_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
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
    // The service runs on its own (as the Downova app or a login item would start it).
    std::fs::create_dir_all(home.path().join("downloads")).unwrap();
    let mut svc = Service(
        Command::new(&service)
            .arg("--data-dir")
            .arg(home.path())
            .arg("--download-dir")
            .arg(home.path().join("downloads"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let host = host_exe();

    let rt = tokio::runtime::Runtime::new().unwrap();
    // Wait (bounded) until the service has published its endpoint: the host never waits for or
    // starts a service, so asking too early would correctly be "unavailable".
    rt.block_on(async {
        let started = Instant::now();
        while !matches!(
            dm_platform::launch::probe(home.path()).await,
            dm_platform::launch::Readiness::Ready(_)
        ) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "dm-service did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let fixture = rt.block_on(dm_test_fixtures::Server::start());
    let body = dm_test_fixtures::payload_seeded(3 * 1024 * 1024, 61);
    fixture.set("public.bin", dm_test_fixtures::Route::new(body.clone()));
    let url = fixture.url("public.bin");
    // The host forwards the add to the running service.
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

    // The host has exited; the service keeps running and completes the transfer.
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
    let _ = svc.wait();
}

// ------------------------------------------------------------------ registration

fn host_cmd(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(host_exe()).args(args).output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Register, repair, status and unregister in an isolated root (and, on Windows, an isolated
/// registry key): the developer's real browser configuration is never touched.
#[test]
fn register_repair_unregister_touch_only_downova_entries() {
    let root = tempfile::tempdir().unwrap();
    let stable = tempfile::tempdir().unwrap();
    let host = isolated_host(stable.path(), None);
    let host_s = host.display().to_string();
    let root_s = root.path().display().to_string();
    let reg_root = format!(r"HKCU\Software\Downova-Test-{}", std::process::id());
    let mut base = vec![
        "--root",
        root_s.as_str(),
        "--browser",
        "chrome",
        "--browser",
        "edge",
    ];
    if cfg!(windows) {
        base.extend(["--registry-root", reg_root.as_str()]);
    }

    let mut register = vec!["register", "--host-path", host_s.as_str()];
    register.extend(base.iter().copied());
    let (code, out, err) = host_cmd(&register);
    println!("{out}{err}");
    assert_eq!(code, Some(0), "{err}");
    let target = |b| dm_browser_host::registration::Target {
        browser: b,
        root: Some(root.path().to_owned()),
        registry_root: Some(reg_root.clone()),
    };
    use dm_browser_host::registration::{Browser, manifest_path};
    let chrome = manifest_path(&target(Browser::Chrome)).unwrap();
    let edge = manifest_path(&target(Browser::Edge)).unwrap();
    for m in [&chrome, &edge] {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(m).unwrap()).unwrap();
        assert_eq!(v["name"], "com.downova.host");
        assert_eq!(v["path"], host_s);
        assert_eq!(v["allowed_origins"], json!([ORIGIN]));
    }
    // Someone else's manifest in the same folder survives everything below.
    let foreign = chrome.parent().unwrap().join("com.example.other.json");
    std::fs::write(&foreign, r#"{"name":"com.example.other"}"#).unwrap();

    // Repair rewrites a damaged manifest that is still ours (wrong origins), and restores a
    // missing one.
    let good = std::fs::read(&chrome).unwrap();
    let mut damaged: Value = serde_json::from_slice(&good).unwrap();
    damaged["allowed_origins"] = json!([]);
    std::fs::write(&chrome, damaged.to_string()).unwrap();
    std::fs::remove_file(&edge).unwrap();
    let mut repair = vec!["repair", "--host-path", host_s.as_str()];
    repair.extend(base.iter().copied());
    assert_eq!(host_cmd(&repair).0, Some(0));
    assert_eq!(std::fs::read(&chrome).unwrap(), good);
    assert!(edge.is_file());
    // An unreadable file's owner is unknown: refused (exit 3, untouched) unless --replace.
    std::fs::write(&chrome, "garbage").unwrap();
    let (code, _, err) = host_cmd(&repair);
    assert_eq!(code, Some(3), "{err}");
    assert!(err.contains("--replace"), "{err}");
    assert_eq!(std::fs::read(&chrome).unwrap(), b"garbage");
    assert!(edge.is_file(), "the other browser was still handled");
    let mut replace = repair.clone();
    replace.push("--replace");
    assert_eq!(host_cmd(&replace).0, Some(0));
    assert_eq!(std::fs::read(&chrome).unwrap(), good);

    let mut status = vec!["status", "--host-path", host_s.as_str(), "--json"];
    status.extend(base.iter().copied());
    let (code, out, _) = host_cmd(&status);
    assert_eq!(code, Some(0));
    let rows: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "{out}");
    for row in &rows {
        assert_eq!(row["ok"], true);
        assert_eq!(row["result"]["owner"], "ours", "{row}");
        if cfg!(windows) {
            assert_eq!(row["result"]["registry_owner"]["owner"], "ours", "{row}");
        }
    }
    if cfg!(windows) {
        let values: Vec<&Value> = rows
            .iter()
            .map(|r| &r["result"]["registry_value"])
            .collect();
        assert_eq!(
            values,
            [
                &json!(chrome.display().to_string()),
                &json!(edge.display().to_string())
            ],
            "registry values point at the manifests"
        );
    }

    // Another installation's uninstall removes nothing of ours.
    let other_dir = tempfile::tempdir().unwrap();
    let other = isolated_host(other_dir.path(), None);
    let other_s = other.display().to_string();
    let mut foreign_uninstall = vec!["unregister", "--host-path", other_s.as_str()];
    foreign_uninstall.extend(base.iter().copied());
    let (code, out, _) = host_cmd(&foreign_uninstall);
    assert_eq!(code, Some(0));
    assert!(out.contains("kept"), "{out}");
    assert!(chrome.exists() && edge.exists());

    let mut unregister = vec!["unregister", "--host-path", host_s.as_str()];
    unregister.extend(base.iter().copied());
    let (code, out, err) = host_cmd(&unregister);
    println!("{out}{err}");
    assert_eq!(code, Some(0));
    assert!(!chrome.exists() && !edge.exists());
    assert!(foreign.exists(), "only Downova's entries are removed");
    // A second unregister is a no-op, not an error.
    assert_eq!(host_cmd(&unregister).0, Some(0));
    if cfg!(windows) {
        let _ = Command::new("reg")
            .args(["delete", &reg_root, "/f"])
            .output();
    }
}

#[test]
fn registration_refuses_unstable_host_paths() {
    let root = tempfile::tempdir().unwrap();
    let root_s = root.path().display().to_string();
    for bad in [
        "relative/dm-browser-host",
        "/tmp/.mount_DownovaX/usr/bin/dm-browser-host",
    ] {
        let (code, _, err) = host_cmd(&[
            "register",
            "--host-path",
            bad,
            "--root",
            &root_s,
            "--registry-root",
            r"HKCU\Software\Downova-Test-never",
        ]);
        assert_eq!(code, Some(3), "{bad}: {err}");
    }
    let (code, _, _) = host_cmd(&["register", "--browser", "netscape"]);
    assert_eq!(code, Some(2));
}
