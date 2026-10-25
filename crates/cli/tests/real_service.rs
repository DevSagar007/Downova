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

fn build_service() {
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
    let mut service = Command::new(exe("dm-service"))
        .arg("--data-dir")
        .arg(data.path())
        .arg("--download-dir")
        .arg(downloads.path())
        .arg("--aria2")
        .arg(&aria2)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

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
    let mut service = Command::new(exe("dm-service"))
        .args(["--backend", "http", "--data-dir"])
        .arg(data.path())
        .arg("--download-dir")
        .arg(downloads.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
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
    let mut service = Command::new(exe("dm-service"))
        .args(["--backend", "http", "--data-dir"])
        .arg(data.path())
        .arg("--download-dir")
        .arg(downloads.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
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
