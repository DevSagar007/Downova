#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use dm_domain::api::{RpcError, RpcRequest, RpcResponse, codes};
use dm_platform::endpoint::{self, EndpointInfo};
use dm_platform::ipc;
use serde_json::json;

use super::*;

/// How the fake service answers one connection.
#[derive(Clone)]
enum Reply {
    Result(Value),
    Error(ErrorData),
    /// Reads the request, then closes without answering (a lost acknowledgement).
    Drop,
}

/// A fake per-user service on the real private IPC transport. Records every request it reads.
struct Fake {
    dir: tempfile::TempDir,
    seen: Arc<Mutex<Vec<RpcRequest>>>,
}

async fn fake(replies: Vec<Reply>) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let run = dm_platform::paths::run_dir(dir.path());
    dm_platform::paths::ensure_private_dir(&run).unwrap();
    let bound = ipc::bind(&run).unwrap();
    endpoint::write(
        dir.path(),
        &EndpointInfo {
            version: 1,
            endpoint: bound.endpoint.clone(),
            pid: std::process::id(),
        },
    )
    .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        for reply in replies {
            let mut conn = bound.accept().await.unwrap();
            let line = conn.read_line().await.unwrap().unwrap();
            let request: RpcRequest = serde_json::from_str(&line).unwrap();
            let id = Some(request.id.clone());
            log.lock().unwrap().push(request);
            match reply {
                Reply::Result(v) => conn.write_json(&RpcResponse::ok(id, v)).await.unwrap(),
                Reply::Error(e) => conn
                    .write_json(&RpcResponse::err(
                        id,
                        RpcError::new(codes::SERVICE_ERROR, e),
                    ))
                    .await
                    .unwrap(),
                Reply::Drop => drop(conn),
            }
        }
    });
    Fake { dir, seen }
}

fn snapshot(job: &str) -> Value {
    json!({
        "job_id": job, "attempt_id": null, "display_name": "f.bin",
        "source_display": "https://h/f.bin", "desired_state": "running", "phase": "queued",
        "blockers": [], "downloaded_bytes": "0", "total_bytes": null, "error": null
    })
}

#[tokio::test]
async fn list_returns_the_contract_snapshots() {
    let f = fake(vec![Reply::Result(
        json!({"items": [snapshot("job_list0001")]}),
    )])
    .await;
    let result = list(f.dir.path()).await.unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].job_id.as_str(), "job_list0001");
    let seen = f.seen.lock().unwrap();
    assert_eq!(seen[0].method, methods::LIST);
}

#[tokio::test]
async fn add_sends_the_canonical_request_with_the_callers_key() {
    let f = fake(vec![Reply::Result(snapshot("job_add00001"))]).await;
    let job = add(
        f.dir.path(),
        "https://example.org/f.bin?sig=SECRET".into(),
        "ui-0123456789".into(),
        AddOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(job.job_id.as_str(), "job_add00001");
    let seen = f.seen.lock().unwrap();
    assert_eq!(seen[0].method, methods::ADD);
    assert_eq!(
        seen[0].params,
        json!({
            "idempotency_key": "ui-0123456789",
            "source": {"url": "https://example.org/f.bin?sig=SECRET"},
            "destination_profile_id": "downloads",
            "expected_size_bytes": null,
            "expected_sha256": null
        })
    );
}

#[tokio::test]
async fn invalid_arguments_are_refused_before_anything_is_sent() {
    let f = fake(vec![]).await;
    for (url, key) in [
        ("ftp://h/f", "ui-0123456789"),
        ("https://u:p@h/f", "ui-0123456789"),
        ("https://h/a b", "ui-0123456789"),
        ("https://h/f", "short"),
        ("https://h/f", "bad key with spaces"),
    ] {
        let e = add(f.dir.path(), url.into(), key.into(), AddOptions::default())
            .await
            .unwrap_err();
        assert_eq!(e.outcome, Outcome::Rejected, "{url} {key}");
        assert_eq!(e.error.kind, ErrorKind::InvalidRequest);
    }
    for job in ["", "att_x", "job_", "../../etc"] {
        let e = control(f.dir.path(), Control::Pause, job)
            .await
            .unwrap_err();
        assert_eq!(e.outcome, Outcome::Rejected, "{job}");
    }
    assert!(f.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn controls_use_their_own_method_and_keep_service_errors() {
    let refused = ErrorData {
        kind: ErrorKind::BackendCapabilityUnavailable,
        retryable: false,
        next_action: NextAction::ChooseRestartOrKeep,
        detail: Some("no strong validator".into()),
    };
    let f = fake(vec![
        Reply::Result(snapshot("job_ctl00001")),
        Reply::Error(refused.clone()),
        Reply::Result(snapshot("job_ctl00001")),
    ])
    .await;
    control(f.dir.path(), Control::Pause, "job_ctl00001")
        .await
        .unwrap();
    let e = control(f.dir.path(), Control::Resume, "job_ctl00001")
        .await
        .unwrap_err();
    assert_eq!(
        e,
        BridgeError {
            outcome: Outcome::Rejected,
            error: refused
        }
    );
    control(f.dir.path(), Control::Cancel, "job_ctl00001")
        .await
        .unwrap();
    let seen = f.seen.lock().unwrap();
    let methods_seen: Vec<_> = seen.iter().map(|r| r.method.as_str()).collect();
    assert_eq!(
        methods_seen,
        [methods::PAUSE, methods::RESUME, methods::CANCEL]
    );
    assert_eq!(seen[2].params, json!({"job_id": "job_ctl00001"}));
}

#[tokio::test]
async fn a_lost_acknowledgement_is_uncertain_for_mutations_only() {
    let f = fake(vec![Reply::Drop, Reply::Drop]).await;
    let e = add(
        f.dir.path(),
        "https://h/f".into(),
        "ui-0123456789".into(),
        AddOptions::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(e.outcome, Outcome::Uncertain);
    assert_eq!(e.error.next_action, NextAction::Retry);
    let e = list(f.dir.path()).await.unwrap_err();
    assert_eq!(e.outcome, Outcome::Unavailable);
}

#[tokio::test]
async fn no_service_is_unavailable_and_nothing_is_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let e = add(
        dir.path(),
        "https://h/f".into(),
        "ui-0123456789".into(),
        AddOptions::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(e.outcome, Outcome::Unavailable);
    assert_eq!(e.error.kind, ErrorKind::ServiceUnavailable);
    let e = control(dir.path(), Control::Cancel, "job_none0001")
        .await
        .unwrap_err();
    assert_eq!(e.outcome, Outcome::Unavailable);
}

#[test]
fn errors_serialize_for_the_ui() {
    let e = BridgeError::uncertain("lost");
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["outcome"], "uncertain");
    assert_eq!(v["error"]["kind"], "SERVICE_UNAVAILABLE");
    assert_eq!(v["error"]["next_action"], "retry");
}

#[tokio::test]
async fn remove_uses_its_own_method_and_validates_the_job_id() {
    let f = fake(vec![Reply::Result(snapshot("job_rm0000001"))]).await;
    assert!(
        control(f.dir.path(), Control::Remove, "../job_rm0000001")
            .await
            .is_err()
    );
    control(f.dir.path(), Control::Remove, "job_rm0000001")
        .await
        .unwrap();
    let seen = f.seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "the invalid ID was never sent");
    assert_eq!(seen[0].method, methods::REMOVE);
    assert_eq!(seen[0].params, json!({"job_id": "job_rm0000001"}));
}

// ------------------------------------------------------------------ B2.8–B2.11 commands

#[tokio::test]
async fn add_options_are_sent_typed_and_only_when_chosen() {
    let f = fake(vec![Reply::Result(snapshot("job_add00002"))]).await;
    add(
        f.dir.path(),
        "https://h/clip.mkv".into(),
        "ui-0123456789".into(),
        AddOptions {
            category: Some("video".into()),
            on_duplicate: Some("refuse".into()),
            queue_id: Some("q_0a1b".into()),
            expected_sha256: Some(format!(" {} ", "AB".repeat(32))),
        },
    )
    .await
    .unwrap();
    let seen = f.seen.lock().unwrap();
    assert_eq!(seen[0].params["expected_sha256"], "ab".repeat(32));
    assert_eq!(seen[0].params["category"], "video");
    assert_eq!(seen[0].params["on_duplicate"], "refuse");
    assert_eq!(seen[0].params["queue_id"], "q_0a1b");
}

#[tokio::test]
async fn closed_values_paths_and_ids_are_refused_before_anything_is_sent() {
    let f = fake(vec![]).await;
    let bad_options = [
        AddOptions {
            expected_sha256: Some("abc".into()),
            ..AddOptions::default()
        },
        AddOptions {
            expected_sha256: Some("z".repeat(64)),
            ..AddOptions::default()
        },
        AddOptions {
            category: Some("photos".into()),
            ..AddOptions::default()
        },
        AddOptions {
            on_duplicate: Some("merge".into()),
            ..AddOptions::default()
        },
        AddOptions {
            queue_id: Some("Q_ABC".into()),
            ..AddOptions::default()
        },
        AddOptions {
            queue_id: Some("../main".into()),
            ..AddOptions::default()
        },
    ];
    for options in bad_options {
        let e = add(
            f.dir.path(),
            "https://h/f".into(),
            "ui-0123456789".into(),
            options.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(e.outcome, Outcome::Rejected, "{options:?}");
    }
    assert!(
        set_priority(f.dir.path(), "job_x1", "urgent")
            .await
            .is_err()
    );
    assert!(move_job(f.dir.path(), "job_x1", "sideways").await.is_err());
    assert!(set_queue(f.dir.path(), "job_x1", "nope").await.is_err());
    assert!(create_queue(f.dir.path(), "   ", None).await.is_err());
    assert!(create_queue(f.dir.path(), "ok", Some(0)).await.is_err());
    assert!(set_queue_limit(f.dir.path(), "main", 17).await.is_err());
    assert!(
        delete_queue(f.dir.path(), "main", Some("bad id"))
            .await
            .is_err()
    );
    for dir in ["relative/dir", "", "/a/../b", "/a/./b"] {
        let settings = ServiceSettings {
            destination_rules: vec![dm_domain::api::DestinationRule {
                category: FileCategory::Video,
                directory: dir.into(),
            }],
            on_collision: dm_domain::policy::CollisionPolicy::Rename,
        };
        assert!(set_settings(f.dir.path(), settings).await.is_err(), "{dir}");
    }
    assert!(
        f.seen.lock().unwrap().is_empty(),
        "nothing reached the service"
    );
}

#[test]
fn queue_ids_follow_the_service_format() {
    for ok in ["main", "q_0", "q_0123456789abcdef"] {
        assert!(is_queue_id(ok), "{ok}");
    }
    for bad in [
        "",
        "Main",
        "q_",
        "q_XYZ",
        "queue",
        "q_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0",
    ] {
        assert!(!is_queue_id(bad), "{bad}");
    }
}

// ------------------------------------------------------------------ B2.16–B2.19 commands

#[tokio::test]
async fn retry_and_replace_source_send_typed_params_and_never_echo_the_link() {
    let f = fake(vec![
        Reply::Result(snapshot("job_rt0000001")),
        Reply::Result(json!({
            "outcome": "choice_required",
            "reason": "content_differs",
            "job": snapshot("job_rt0000001"),
        })),
    ])
    .await;
    retry(
        f.dir.path(),
        "job_rt0000001",
        "op_ui_00000001".into(),
        Some("att_0000000000000001".into()),
        true,
    )
    .await
    .unwrap();
    let r = replace_source(
        f.dir.path(),
        "job_rt0000001",
        "op_ui_00000002".into(),
        "https://h/a.bin?sig=SECRET".into(),
        "keep_if_verified",
        None,
    )
    .await
    .unwrap();
    assert_eq!(r.outcome, dm_domain::api::ReplaceOutcome::ChoiceRequired);
    let seen = f.seen.lock().unwrap();
    assert_eq!(seen[0].method, methods::RETRY);
    assert_eq!(seen[0].params["discard_partial"], true);
    assert_eq!(
        seen[0].params["expected_attempt_id"],
        "att_0000000000000001"
    );
    assert_eq!(seen[1].method, methods::REPLACE_SOURCE);
    assert_eq!(seen[1].params["mode"], "keep_if_verified");
}

#[tokio::test]
async fn bad_operation_ids_modes_and_links_are_refused_before_sending() {
    let f = fake(vec![]).await;
    assert!(
        retry(f.dir.path(), "job_rt0000001", "short".into(), None, false)
            .await
            .is_err()
    );
    assert!(
        retry(
            f.dir.path(),
            "job_rt0000001",
            "op_ui_00000003".into(),
            Some("gid-1".into()),
            false
        )
        .await
        .is_err()
    );
    for (url, mode) in [
        ("https://h/a.bin", "splice"),
        ("file:///C:/Windows/notepad.exe", "restart"),
    ] {
        let e = replace_source(
            f.dir.path(),
            "job_rt0000001",
            "op_ui_00000004".into(),
            url.into(),
            mode,
            None,
        )
        .await
        .unwrap_err();
        assert!(!format!("{e:?}").contains("notepad"));
    }
    assert!(f.seen.lock().unwrap().is_empty());
}
