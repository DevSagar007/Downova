//! CLI behaviour against an isolated IPC test peer (no service, no engine) and the binary itself.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::print_stdout
)]

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dm_cli::{CliOutput, RunOptions, exit, run_cli};
use dm_domain::ErrorKind;
use dm_domain::api::{codes, methods};
use serde_json::{Value, json};
use support::{Reply, happy, hello, rpc_error, snapshot};

const SECRET: &str = "SYNTHETIC-SECRET-b03";

fn secret_url() -> String {
    format!("https://user:{SECRET}@cdn.example.org/file.zip?token={SECRET}")
}

fn opts() -> RunOptions {
    RunOptions {
        call_timeout: Duration::from_secs(5),
        retry_add_once: true,
    }
}

async fn dm(data: &Path, args: &[&str]) -> CliOutput {
    let mut all: Vec<OsString> = vec!["--data-dir".into(), data.into()];
    all.extend(args.iter().map(OsString::from));
    run_cli(&all, &opts()).await
}

fn json_out(out: &CliOutput) -> Value {
    serde_json::from_str(out.stdout.trim())
        .unwrap_or_else(|_| panic!("stdout is not one JSON object: {}", out.stdout))
}

fn assert_no_secret(out: &CliOutput) {
    for text in [&out.stdout, &out.stderr] {
        assert!(
            !text.contains(SECRET) && !text.contains("user:"),
            "secret leaked: {text}"
        );
    }
}

// ---------- the binary ----------

fn bin(args: &[&str], home: Option<&Path>) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_dm"));
    cmd.args(args);
    if let Some(home) = home {
        cmd.env("DOWNOVA_HOME", home);
    }
    cmd.output().unwrap()
}

#[test]
fn binary_help_version_and_usage_exit_codes() {
    let help = bin(&["--help"], None);
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("EXIT CODES") && text.contains("--idempotency-key"));

    let version = bin(&["--version", "--json"], None);
    assert_eq!(version.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&version.stdout).unwrap();
    assert_eq!(v["data"]["api_version"], 1);

    let bad = bin(&["add", &secret_url(), "--bogus"], None);
    assert_eq!(bad.status.code(), Some(i32::from(exit::USAGE)));
    assert!(bad.stdout.is_empty(), "human errors go to stderr");
    assert!(!String::from_utf8_lossy(&bad.stderr).contains(SECRET));
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_uses_the_default_data_dir_from_dm_platform() {
    // DOWNOVA_HOME is the documented override of dm_platform::paths::data_dir().
    let empty = tempfile::tempdir().unwrap();
    let path = empty.path().to_owned();
    let out = tokio::task::spawn_blocking(move || bin(&["list"], Some(&path)))
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(i32::from(exit::UNAVAILABLE)));
    assert!(String::from_utf8_lossy(&out.stderr).contains("SERVICE_UNAVAILABLE"));

    let home = tempfile::tempdir().unwrap();
    let log = support::start(home.path(), happy());
    let path = home.path().to_owned();
    let out = tokio::task::spawn_blocking(move || bin(&["list", "--json"], Some(&path)))
        .await
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(log.methods(), vec![methods::HELLO, methods::LIST]);
}

// ---------- arguments and unavailable service ----------

#[tokio::test]
async fn usage_errors_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let human = dm(dir.path(), &["get", "not-a-job"]).await;
    assert_eq!(human.exit, exit::USAGE);
    assert!(human.stdout.is_empty() && human.stderr.contains("JOB-ID"));

    let json = dm(dir.path(), &["--json", "add"]).await;
    assert_eq!(json.exit, exit::USAGE);
    let v = json_out(&json);
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "add");
    assert_eq!(v["exit_code"], 2);
    assert_eq!(v["error"]["kind"], "INVALID_REQUEST");
}

#[tokio::test]
async fn missing_and_stale_endpoints_are_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let out = dm(dir.path(), &["--json", "list"]).await;
    assert_eq!(out.exit, exit::UNAVAILABLE);
    assert_eq!(json_out(&out)["error"]["kind"], "SERVICE_UNAVAILABLE");

    dm_platform::paths::ensure_private_dir(&dm_platform::paths::run_dir(dir.path())).unwrap();
    let stale = if cfg!(windows) {
        "downova-0000000000000000"
    } else {
        "/nonexistent/downova.sock"
    };
    dm_platform::endpoint::write(
        dir.path(),
        &dm_platform::endpoint::EndpointInfo {
            version: 1,
            endpoint: stale.into(),
            pid: 1,
        },
    )
    .unwrap();
    let out = dm(dir.path(), &["list"]).await;
    assert_eq!(out.exit, exit::UNAVAILABLE);
    assert!(out.stderr.contains("stale endpoint"), "{}", out.stderr);
}

// ---------- handshake ----------

#[tokio::test]
async fn incompatible_api_version_stops_before_any_application_call() {
    let dir = tempfile::tempdir().unwrap();
    let log = support::start(dir.path(), Arc::new(|_, _| Reply::Result(hello(2))));
    let out = dm(dir.path(), &["--json", "add", "https://h/f.zip"]).await;
    assert_eq!(out.exit, exit::UNAVAILABLE);
    assert!(
        json_out(&out)["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("incompatible")
    );
    assert_eq!(log.methods(), vec![methods::HELLO]);
}

// ---------- command mapping and presentation ----------

#[tokio::test]
async fn every_command_maps_to_its_method_and_params() {
    let dir = tempfile::tempdir().unwrap();
    let log = support::start(dir.path(), happy());
    for (args, method) in [
        (vec!["list"], methods::LIST),
        (vec!["get", "job_aaa"], methods::GET),
        (vec!["pause", "job_aaa"], methods::PAUSE),
        (vec!["resume", "job_aaa"], methods::RESUME),
        (vec!["cancel", "job_aaa"], methods::CANCEL),
        (
            vec![
                "add",
                "https://h/f.zip",
                "--profile",
                "work",
                "--idempotency-key",
                "key-0000001",
            ],
            methods::ADD,
        ),
    ] {
        let out = dm(dir.path(), &args).await;
        assert_eq!(out.exit, exit::OK, "{args:?}: {}", out.stderr);
        let last = log.app_requests().pop().unwrap();
        assert_eq!(last.method, method);
        if method != methods::LIST && method != methods::ADD {
            assert_eq!(last.params, json!({"job_id": "job_aaa"}));
        }
    }
    let add = log.app_requests().pop().unwrap();
    assert_eq!(add.params["destination_profile_id"], "work");
    assert_eq!(add.params["idempotency_key"], "key-0000001");
    // Every connection starts with hello.
    assert!(
        log.requests()
            .iter()
            .filter(|(_, r)| r.method == methods::HELLO)
            .count()
            >= 6
    );
}

#[tokio::test]
async fn human_and_json_success_output() {
    let dir = tempfile::tempdir().unwrap();
    support::start(dir.path(), happy());

    let human = dm(dir.path(), &["get", "job_peer0001"]).await;
    assert_eq!(human.exit, exit::OK);
    for expected in [
        "job_peer0001",
        "running (phase: transferring)",
        "1.0 MiB of 2.0 MiB (50.0%)",
        "metered network",
        "att_peer0001",
    ] {
        assert!(
            human.stdout.contains(expected),
            "missing {expected:?} in\n{}",
            human.stdout
        );
    }
    assert!(human.stderr.is_empty());

    let list = dm(dir.path(), &["list"]).await;
    assert!(list.stdout.contains("JOB ID") && list.stdout.contains("job_peer0001"));

    let json = dm(dir.path(), &["add", "--json", "https://h/f.zip"]).await;
    let v = json_out(&json);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "add");
    assert_eq!(v["data"]["job_id"], "job_peer0001");
    assert!(v["idempotency_key"].as_str().unwrap().starts_with("cli-"));
    assert!(json.stderr.is_empty());
}

#[tokio::test]
async fn urls_with_secrets_are_redacted_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let leaky = Arc::new(
        |req: &dm_domain::api::RpcRequest, _: usize| match req.method.as_str() {
            methods::HELLO => Reply::Result(hello(1)),
            methods::ADD => {
                // A hostile/buggy peer echoing the raw URL must still not reach the terminal.
                let mut s = snapshot("job_peer0001");
                s["source_display"] = json!(secret_url());
                s["display_name"] = json!(format!("from {}", secret_url()));
                Reply::Result(s)
            }
            _ => Reply::Error(rpc_error(
                codes::SERVICE_ERROR,
                ErrorKind::AuthRequired,
                &format!("denied for {}", secret_url()),
            )),
        },
    );
    support::start(dir.path(), leaky);
    let url = secret_url();
    for args in [
        vec!["add", url.as_str()],
        vec!["--json", "add", url.as_str()],
        vec!["get", "job_x1"],
        vec!["--json", "get", "job_x1"],
    ] {
        let out = dm(dir.path(), &args).await;
        assert_no_secret(&out);
    }
}

// ---------- idempotency, retries and failures ----------

#[tokio::test]
async fn lost_acknowledgement_is_retried_once_with_the_same_key() {
    let dir = tempfile::tempdir().unwrap();
    let script =
        Arc::new(
            |req: &dm_domain::api::RpcRequest, conn: usize| match req.method.as_str() {
                methods::HELLO => Reply::Result(hello(1)),
                methods::ADD if conn == 1 => Reply::Drop,
                _ => Reply::Result(snapshot("job_peer0001")),
            },
        );
    let log = support::start(dir.path(), script);
    let out = dm(dir.path(), &["--json", "add", "https://h/f.zip"]).await;
    assert_eq!(out.exit, exit::OK, "{}", out.stdout);
    let adds: Vec<_> = log
        .app_requests()
        .into_iter()
        .filter(|r| r.method == methods::ADD)
        .collect();
    assert_eq!(adds.len(), 2, "one retry");
    assert_eq!(
        adds[0].params["idempotency_key"],
        adds[1].params["idempotency_key"]
    );
    assert_eq!(
        json_out(&out)["idempotency_key"],
        adds[0].params["idempotency_key"]
    );
}

#[tokio::test]
async fn explicit_key_is_reused_across_invocations() {
    let dir = tempfile::tempdir().unwrap();
    let log = support::start(dir.path(), happy());
    for _ in 0..2 {
        assert_eq!(
            dm(
                dir.path(),
                &[
                    "add",
                    "https://h/f.zip",
                    "--idempotency-key",
                    "script-key-1"
                ]
            )
            .await
            .exit,
            exit::OK
        );
    }
    let keys: Vec<_> = log
        .app_requests()
        .into_iter()
        .map(|r| r.params["idempotency_key"].clone())
        .collect();
    assert_eq!(keys, vec![json!("script-key-1"), json!("script-key-1")]);
}

#[tokio::test]
async fn timeout_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    support::start(
        dir.path(),
        Arc::new(
            |req: &dm_domain::api::RpcRequest, _: usize| match req.method.as_str() {
                methods::HELLO => Reply::Result(hello(1)),
                _ => Reply::Hang,
            },
        ),
    );
    let started = std::time::Instant::now();
    let fast = RunOptions {
        call_timeout: Duration::from_millis(300),
        retry_add_once: false,
    };
    let args: Vec<OsString> = vec!["--data-dir".into(), dir.path().into(), "list".into()];
    let out = run_cli(&args, &fast).await;
    assert_eq!(out.exit, exit::UNAVAILABLE);
    assert!(out.stderr.contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn rejected_and_unexpected_responses_map_to_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    support::start(
        dir.path(),
        Arc::new(|req: &dm_domain::api::RpcRequest, _: usize| {
            let job = req.params["job_id"].as_str().unwrap_or_default().to_owned();
            match (req.method.as_str(), job.as_str()) {
                (methods::HELLO, _) => Reply::Result(hello(1)),
                (methods::ADD, _) => Reply::Error(rpc_error(
                    codes::SERVICE_ERROR,
                    ErrorKind::BackendCapabilityUnavailable,
                    "Http is Unverified for this backend",
                )),
                (_, "job_conflict") => Reply::Error(rpc_error(
                    codes::SERVICE_ERROR,
                    ErrorKind::IdempotencyConflict,
                    "different request",
                )),
                (_, "job_unknown") => Reply::Error(rpc_error(
                    codes::INVALID_PARAMS,
                    ErrorKind::InvalidRequest,
                    "unknown job job_unknown",
                )),
                (_, "job_malformed") => Reply::Result(json!({"surprise": true})),
                (_, "job_internal") => {
                    Reply::Error(rpc_error(codes::SERVICE_ERROR, ErrorKind::Internal, "boom"))
                }
                _ => Reply::Error(rpc_error(
                    codes::METHOD_NOT_FOUND,
                    ErrorKind::InvalidRequest,
                    "unknown method",
                )),
            }
        }),
    );

    let refused = dm(dir.path(), &["--json", "add", "https://h/f.zip"]).await;
    assert_eq!(refused.exit, exit::REJECTED);
    let v = json_out(&refused);
    assert_eq!(v["error"]["kind"], "BACKEND_CAPABILITY_UNAVAILABLE");
    assert_eq!(v["error"]["rpc_code"], -32000);

    let human = dm(dir.path(), &["add", "https://h/f.zip"]).await;
    assert_eq!(human.exit, exit::REJECTED);
    assert!(human.stdout.is_empty() && human.stderr.contains("BACKEND_CAPABILITY_UNAVAILABLE"));

    assert_eq!(
        dm(dir.path(), &["get", "job_conflict"]).await.exit,
        exit::REJECTED
    );
    assert_eq!(
        dm(dir.path(), &["get", "job_unknown"]).await.exit,
        exit::REJECTED
    );
    assert_eq!(
        dm(dir.path(), &["get", "job_malformed"]).await.exit,
        exit::UNEXPECTED
    );
    assert_eq!(
        dm(dir.path(), &["get", "job_internal"]).await.exit,
        exit::UNEXPECTED
    );
    assert_eq!(
        dm(dir.path(), &["get", "job_other"]).await.exit,
        exit::UNEXPECTED
    );
}
