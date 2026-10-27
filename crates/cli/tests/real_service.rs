//! `dm` against the real `dm-service` + aria2 (B0.3). Needs aria2c, so it is `#[ignore]`d in plain
//! `cargo test` and run by `pnpm verify` (group `aria2`). dm-cli does not depend on dm-service;
//! the test builds the service binary with the same cargo and runs it as a separate process.

#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const SECRET: &str = "SYNTHETIC-SECRET-b03-real";

fn exe(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_BIN_EXE_dm"))
        .parent()
        .unwrap()
        .to_owned();
    dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn aria2() -> Option<PathBuf> {
    let name = format!("aria2c{}", std::env::consts::EXE_SUFFIX);
    std::env::var_os("DM_ARIA2_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([workspace().join("tools/aria2").join(&name)])
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join(&name)),
        )
        .find(|p| p.is_file())
}

/// A service started by a test. Dropping it (success, panic or timeout) kills and reaps it if it
/// is still running, so no test leaves a service holding `target/debug/dm-service` open (which
/// makes later `cargo build`s fail with "access denied" on Windows). Only this child is touched.
struct Service(Child);

impl std::ops::Deref for Service {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Service {
    fn deref_mut(&mut self) -> &mut Child {
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

/// Builds dm-service once per test binary, before any test spawns it: a later build must never
/// try to replace the executable while another test's service is running from it.
fn build_service() {
    static BUILT: std::sync::Once = std::sync::Once::new();
    BUILT.call_once(build_service_now);
}

fn build_service_now() {
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
    assert!(status.success(), "building dm-service failed");
}

fn dm(data: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_dm"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn payload(len: usize) -> Vec<u8> {
    let mut state: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xFF) as u8
        })
        .collect()
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Minimal controlled HTTP fixture on a background thread: 200 + Content-Length for any path.
fn fixture(body: Vec<u8>) -> std::net::SocketAddr {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { return };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(b[0]);
                }
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s
                    .write_all(hdr.as_bytes())
                    .and_then(|()| s.write_all(&body));
            });
        }
    });
    addr
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
#[ignore = "needs aria2c; run via `pnpm verify` (group aria2)"]
fn cli_against_real_service_and_aria2() {
    let aria2 = aria2().expect("aria2c must be available");
    build_service();
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = Service(
        Command::new(exe("dm-service"))
            .arg("--data-dir")
            .arg(data.path())
            .arg("--download-dir")
            .arg(downloads.path())
            .args(["--backend", "aria2", "--aria2"])
            .arg(&aria2)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    // Wait for the service (hello is part of every dm invocation).
    let started = Instant::now();
    let list = loop {
        let (code, out, _) = dm(data.path(), &["--json", "list"]);
        if code == 0 {
            break out;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not come up"
        );
        std::thread::sleep(Duration::from_millis(150));
    };
    let v: Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(
        v["data"]["items"].as_array().unwrap().len(),
        0,
        "empty list"
    );

    let (code, out, err) = dm(data.path(), &["get", "job_unknown01"]);
    println!("get unknown → exit {code}: {}", err.trim());
    assert_eq!(code, 4);
    assert!(out.is_empty() && err.contains("INVALID_REQUEST"));

    // HTTPS stays Unverified (ADR 0003): refused honestly, with no job and no file.
    let url = format!("https://127.0.0.1:9/file.bin?token={SECRET}");
    let (code, out, err) = dm(
        data.path(),
        &["--json", "add", &url, "--idempotency-key", "real-cli-key-1"],
    );
    println!("https add → exit {code}: {}", out.trim());
    assert_eq!(code, 4);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["error"]["kind"], "BACKEND_CAPABILITY_UNAVAILABLE");
    assert!(!out.contains(SECRET) && !err.contains(SECRET));
    let (code, human_out, human_err) = dm(data.path(), &["add", &url]);
    assert_eq!(code, 4);
    assert!(!human_out.contains(SECRET) && !human_err.contains(SECRET));
    let (_, list, _) = dm(data.path(), &["--json", "list"]);
    let v: Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(v["data"]["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        std::fs::read_dir(downloads.path()).unwrap().count(),
        0,
        "no file written"
    );

    // HTTP is Supported (ADR 0003): a real download from a controlled fixture via `dm add`.
    let body = payload(2 * 1024 * 1024);
    let addr = fixture(body.clone());
    let url = format!("http://{addr}/files/data.bin?token={SECRET}");
    let (code, out, err) = dm(data.path(), &["--json", "add", &url]);
    println!("http add → exit {code}: {}", out.trim());
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains(SECRET), "source_display is redacted");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let job = v["data"]["job_id"].as_str().unwrap().to_owned();
    let started = Instant::now();
    let done = loop {
        let (_, out, _) = dm(data.path(), &["--json", "get", &job]);
        let v: Value = serde_json::from_str(out.trim()).unwrap();
        let phase = v["data"]["phase"].as_str().unwrap_or_default().to_owned();
        if ["completed", "failed", "cancelled"].contains(&phase.as_str())
            || started.elapsed() > Duration::from_secs(60)
        {
            break v;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(done["data"]["phase"], "completed", "{done}");
    let name = done["data"]["display_name"].as_str().unwrap();
    let file = std::fs::read(downloads.path().join(name)).unwrap();
    println!(
        "downloaded {name}: {} bytes, sha256 {}",
        file.len(),
        sha_hex(&file)
    );
    assert_eq!(
        sha_hex(&file),
        sha_hex(&body),
        "SHA-256 of the completed file"
    );
    let (code, human, _) = dm(data.path(), &["get", &job]);
    assert_eq!(code, 0);
    assert!(human.contains("(phase: completed)") && !human.contains(SECRET));

    // dm has exited many times; the service is still running.
    assert!(
        service.try_wait().unwrap().is_none(),
        "service must outlive the CLI"
    );

    // Stop the service through the API (the CLI has no shutdown command by design).
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut client = dm_platform::ipc::Client::connect(data.path())
            .await
            .unwrap();
        client
            .call(dm_domain::api::methods::SHUTDOWN, serde_json::json!({}))
            .await
            .unwrap();
    });
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
    let (code, _, _) = dm(data.path(), &["list"]);
    assert_eq!(code, 3, "unavailable after shutdown");
}

/// `dm` against `dm-service --backend http` (B0.4.2, ADR 0004): no engine binary needed, but the
/// test builds the service with cargo like the aria2 one, so it runs in the same verify step.
#[test]
#[ignore = "builds dm-service; run via `pnpm verify` (group aria2, CLI real-service step)"]
fn cli_against_real_service_with_http_backend() {
    build_service();
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = Service(
        Command::new(exe("dm-service"))
            .args(["--backend", "http", "--data-dir"])
            .arg(data.path())
            .arg("--download-dir")
            .arg(downloads.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while dm(data.path(), &["--json", "list"]).0 != 0 {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not come up"
        );
        std::thread::sleep(Duration::from_millis(150));
    }

    let body = payload(2 * 1024 * 1024);
    let addr = fixture(body.clone());
    let url = format!("http://{addr}/files/http-backend.bin?token={SECRET}");
    let (code, out, err) = dm(data.path(), &["--json", "add", &url]);
    println!("http backend add → exit {code}: {}", out.trim());
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains(SECRET));
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let job = v["data"]["job_id"].as_str().unwrap().to_owned();
    let started = Instant::now();
    let done = loop {
        let (_, out, _) = dm(data.path(), &["--json", "get", &job]);
        let v: Value = serde_json::from_str(out.trim()).unwrap();
        let phase = v["data"]["phase"].as_str().unwrap_or_default().to_owned();
        if ["completed", "failed", "cancelled"].contains(&phase.as_str())
            || started.elapsed() > Duration::from_secs(60)
        {
            break v;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(done["data"]["phase"], "completed", "{done}");
    let name = done["data"]["display_name"].as_str().unwrap();
    let file = std::fs::read(downloads.path().join(name)).unwrap();
    println!(
        "http backend downloaded {name}: {} bytes, sha256 {}",
        file.len(),
        sha_hex(&file)
    );
    assert_eq!(
        sha_hex(&file),
        sha_hex(&body),
        "SHA-256 of the completed file"
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut client = dm_platform::ipc::Client::connect(data.path())
            .await
            .unwrap();
        client
            .call(dm_domain::api::methods::SHUTDOWN, serde_json::json!({}))
            .await
            .unwrap();
    });
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}

/// `dm pause` / `dm resume` against `dm-service --backend http` (B0.4.2, validated resume): the
/// resumed file must be byte-identical (SHA-256) and the continuation must be a validated range.
#[test]
#[ignore = "builds dm-service; run via `pnpm verify` (group aria2, CLI real-service step)"]
fn cli_pause_resume_with_http_backend() {
    build_service();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let body = dm_test_fixtures::payload_seeded(6 * 1024 * 1024, 41);
    let server = rt.block_on(dm_test_fixtures::Server::start());
    let mut route = dm_test_fixtures::Route::new(body.clone());
    route.slow = Some(Duration::from_millis(5));
    server.set("pr.bin", route);

    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut service = Service(
        Command::new(exe("dm-service"))
            .args(["--backend", "http", "--data-dir"])
            .arg(data.path())
            .arg("--download-dir")
            .arg(downloads.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while dm(data.path(), &["--json", "list"]).0 != 0 {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "service did not come up"
        );
        std::thread::sleep(Duration::from_millis(150));
    }

    let get = |job: &str| -> Value {
        let (_, out, _) = dm(data.path(), &["--json", "get", job]);
        serde_json::from_str::<Value>(out.trim()).unwrap()["data"].clone()
    };
    let (code, out, err) = dm(data.path(), &["--json", "add", &server.url("pr.bin")]);
    assert_eq!(code, 0, "{err}");
    let job = serde_json::from_str::<Value>(out.trim()).unwrap()["data"]["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = Instant::now();
    while get(&job)["downloaded_bytes"]
        .as_str()
        .unwrap_or("0")
        .parse::<u64>()
        .unwrap()
        < 512 * 1024
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(50));
    }
    let (code, out, _) = dm(data.path(), &["--json", "pause", &job]);
    println!("dm pause → exit {code}: {}", out.trim());
    assert_eq!(code, 0);
    server.update("pr.bin", |r| r.slow = None);
    let (code, out, _) = dm(data.path(), &["--json", "resume", &job]);
    println!("dm resume → exit {code}: {}", out.trim());
    assert_eq!(code, 0);
    let started = Instant::now();
    let done = loop {
        let v = get(&job);
        let phase = v["phase"].as_str().unwrap_or_default().to_owned();
        if ["completed", "failed", "cancelled"].contains(&phase.as_str())
            || started.elapsed() > Duration::from_secs(60)
        {
            break v;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let file = std::fs::read(
        downloads
            .path()
            .join(done["display_name"].as_str().unwrap()),
    )
    .unwrap();
    let log = server.log_for("pr.bin");
    let ranges: Vec<String> = log
        .iter()
        .filter_map(|l| l.header("range").map(str::to_owned))
        .collect();
    println!(
        "dm pause/resume: phase {}, {} requests, ranges {ranges:?}, sha256 {}",
        done["phase"],
        log.len(),
        sha_hex(&file)
    );
    assert_eq!(done["phase"], "completed", "{done}");
    assert_eq!(sha_hex(&file), sha_hex(&body), "SHA-256 after pause/resume");
    // B0.4.3: 6 MiB over 4 validated segments; after `dm resume`, continuation ranges start
    // inside segments, and every ranged request carries If-Range.
    let boundaries = [
        "bytes=0-1572863",
        "bytes=1572864-3145727",
        "bytes=3145728-4718591",
        "bytes=4718592-6291455",
    ];
    assert!(
        boundaries.iter().all(|b| ranges.iter().any(|r| r == b)),
        "four segments: {ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| !boundaries.contains(&r.as_str())),
        "a real continuation: {ranges:?}"
    );
    assert!(
        log.iter()
            .filter(|l| l.header("range").is_some())
            .all(|l| l.header("if-range").is_some())
    );

    rt.block_on(async {
        let mut client = dm_platform::ipc::Client::connect(data.path())
            .await
            .unwrap();
        client
            .call(dm_domain::api::methods::SHUTDOWN, serde_json::json!({}))
            .await
            .unwrap();
    });
    assert_eq!(wait_exit(&mut service, Duration::from_secs(20)), Some(0));
}

/// B2.4: `dm remove` and `dm shutdown --pause-downloads` against `dm-service --backend http`.
/// Removing keeps the file and the job stays out of the list after a restart; a paused shutdown
/// is verified as stopped, the restarted service keeps the job paused, and `dm resume` completes
/// it byte-exact (ADR 0012, ADR 0013).
#[test]
#[ignore = "builds dm-service; run via `pnpm verify` (group aria2, CLI real-service step)"]
fn cli_remove_and_paused_shutdown_with_http_backend() {
    build_service();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let small = dm_test_fixtures::payload_seeded(256 * 1024, 51);
    let big = dm_test_fixtures::payload_seeded(6 * 1024 * 1024, 52);
    let server = rt.block_on(dm_test_fixtures::Server::start());
    server.set("small.bin", dm_test_fixtures::Route::new(small.clone()));
    let mut slow = dm_test_fixtures::Route::new(big.clone());
    slow.slow = Some(Duration::from_millis(20));
    server.set("big.bin", slow);

    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let spawn = || {
        Service(
            Command::new(exe("dm-service"))
                .args(["--backend", "http", "--data-dir"])
                .arg(data.path())
                .arg("--download-dir")
                .arg(downloads.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let ready = || {
        let started = Instant::now();
        while dm(data.path(), &["--json", "list"]).0 != 0 {
            assert!(started.elapsed() < Duration::from_secs(20), "no service");
            std::thread::sleep(Duration::from_millis(150));
        }
    };
    let get = |job: &str| -> Value {
        let (_, out, _) = dm(data.path(), &["--json", "get", job]);
        serde_json::from_str::<Value>(out.trim()).unwrap()["data"].clone()
    };
    let add = |url: &str| -> String {
        let (code, out, err) = dm(data.path(), &["--json", "add", url]);
        assert_eq!(code, 0, "{err}");
        serde_json::from_str::<Value>(out.trim()).unwrap()["data"]["job_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let wait_phase = |job: &str, phase: &str| -> Value {
        let started = Instant::now();
        loop {
            let v = get(job);
            if v["phase"] == phase {
                return v;
            }
            assert!(started.elapsed() < Duration::from_secs(60), "{v}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let listed = || -> Vec<String> {
        let (_, out, _) = dm(data.path(), &["--json", "list"]);
        serde_json::from_str::<Value>(out.trim()).unwrap()["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["job_id"].as_str().unwrap().to_owned())
            .collect()
    };

    let mut service = spawn();
    ready();
    let done = add(&server.url("small.bin"));
    let finished = wait_phase(&done, "completed");
    let file = downloads
        .path()
        .join(finished["display_name"].as_str().unwrap());
    let running = add(&server.url("big.bin"));
    let started = Instant::now();
    while get(&running)["downloaded_bytes"]
        .as_str()
        .unwrap_or("0")
        .parse::<u64>()
        .unwrap()
        == 0
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(50));
    }

    // Unfinished jobs are refused; a finished one leaves the list and keeps its file.
    let (code, out, _) = dm(data.path(), &["--json", "remove", &running]);
    assert_eq!(code, 4, "{out}");
    let (code, out, err) = dm(data.path(), &["--json", "remove", &done]);
    println!("dm remove → exit {code}: {}", out.trim());
    assert_eq!(code, 0, "{err}");
    assert!(!listed().contains(&done));
    assert_eq!(sha_hex(&std::fs::read(&file).unwrap()), sha_hex(&small));

    // Exit and stop downloads from the CLI.
    let (code, out, err) = dm(data.path(), &["--json", "shutdown", "--pause-downloads"]);
    println!(
        "dm shutdown --pause-downloads → exit {code}: {}",
        out.trim()
    );
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        v["data"],
        serde_json::json!({"mode": "pause_downloads", "paused_jobs": 1, "stopped": true})
    );
    assert_eq!(wait_exit(&mut service.0, Duration::from_secs(20)), Some(0));

    let _service = spawn();
    ready();
    assert_eq!(listed(), vec![running.clone()], "the removed job stays out");
    assert_eq!(get(&done)["removed"], true);
    let paused = get(&running);
    assert_eq!(paused["desired_state"], "paused");
    let at = paused["downloaded_bytes"].clone();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(get(&running)["downloaded_bytes"], at, "stays paused");
    server.update("big.bin", |r| r.slow = None);
    let (code, _, err) = dm(data.path(), &["--json", "resume", &running]);
    assert_eq!(code, 0, "{err}");
    let finished = wait_phase(&running, "completed");
    let resumed = std::fs::read(
        downloads
            .path()
            .join(finished["display_name"].as_str().unwrap()),
    )
    .unwrap();
    assert_eq!(sha_hex(&resumed), sha_hex(&big));
    assert_eq!(sha_hex(&std::fs::read(&file).unwrap()), sha_hex(&small));
    println!("dm remove + paused shutdown + resume: both files SHA-256 exact");
}
