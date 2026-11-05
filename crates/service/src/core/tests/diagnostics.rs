//! E05/SEC-01 (ADR 0026): synthetic secrets seeded into every input the service holds never reach
//! the diagnostics preview, the exported file or the event stream; useful non-secret facts do.
use super::*;
use dm_domain::api::{DiagnosticsExportParams, DiagnosticsPreviewParams};

const SECRET: &str = "SYNTH-SECRET-e05";

fn secret_inputs() -> Vec<String> {
    vec![
        format!("https://user:{SECRET}@cdn.example.org/a.zip"),
        format!("https://cdn.example.org/b.zip?token={SECRET}&sig={SECRET}"),
        format!("https://cdn.example.org/{SECRET}/c.zip#{SECRET}"),
        format!("https://cdn.example.org/d.zip?X-Amz-Signature={SECRET}"),
    ]
}

async fn seeded(dir: &Path) -> (Core<FakeBackend>, Vec<DownloadSnapshot>) {
    let core = core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir);
    core.create_queue(&format!("Queue {SECRET}"), None)
        .await
        .unwrap();
    let mut jobs = Vec::new();
    for (i, url) in secret_inputs().into_iter().enumerate() {
        jobs.push(
            core.add(add_params(&format!("diag-secret-{i:04}"), &url))
                .await
                .unwrap(),
        );
    }
    // A failure whose engine detail carries every kind of credential text.
    let failing = &jobs[1];
    core.backend().set_progress(
        &handle_of(failing),
        progress(
            TransferState::Failed(BackendError::new(
                ErrorKind::AuthRequired,
                format!(
                    "Authorization: Bearer {SECRET}; Cookie: sid={SECRET}; Proxy-Authorization: Basic {SECRET}; via http://u:{SECRET}@proxy.example:8080; saved to C:\\Users\\someone\\Downloads"
                ),
            )),
            0,
            None,
        ),
    );
    core.poll_once().await;
    (core, jobs)
}

#[tokio::test]
async fn synthetic_secrets_never_reach_the_preview_the_export_or_events() {
    let dir = tempfile::tempdir().unwrap();
    let (core, jobs) = seeded(dir.path()).await;
    let mut events = core.subscribe().await.events;
    let preview = core
        .diagnostics_preview(DiagnosticsPreviewParams {
            include_crash: true,
        })
        .await
        .unwrap();
    let text = &preview.content;
    for needle in [
        SECRET,
        "cdn.example.org",
        "Authorization",
        "Bearer",
        "Cookie",
        "proxy.example",
        "someone",
        "a.zip",
        "Queue ",
        dir.path().to_str().unwrap(),
    ] {
        assert!(!text.contains(needle), "{needle} leaked into:\n{text}");
    }
    assert!(dm_domain::redact::export_leaks(text).is_empty());
    // Useful, non-secret facts are kept.
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["backend"]["id"], "fake");
    assert_eq!(v["jobs"]["total"], 4);
    assert_eq!(v["settings"]["queues"], 2);
    assert_eq!(v["recent_failures"][0]["error_kind"], "AUTH_REQUIRED");
    assert_eq!(v["recent_failures"][0]["job_id"], jobs[1].job_id.as_str());
    assert!(
        v["crash"]["last_startup"].is_object(),
        "opt-in crash section"
    );
    assert!(v["limitations"].as_array().unwrap().len() >= 3);
    // Without the opt-in there is no crash section.
    let plain = core
        .diagnostics_preview(DiagnosticsPreviewParams::default())
        .await
        .unwrap();
    assert!(!plain.content.contains("\"crash\""));
    // The export writes exactly the previewed snapshot.
    let out = dir.path().join("support.json");
    let r = core
        .diagnostics_export(DiagnosticsExportParams {
            snapshot_id: plain.snapshot_id.clone(),
            path: out.display().to_string(),
            replace: false,
        })
        .await
        .unwrap();
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written, plain.content, "byte-identical to the preview");
    assert_eq!(r.size_bytes, plain.size_bytes);
    assert!(!written.contains(SECRET));
    // Collection emitted no events (read-only) and no event carries a secret.
    while let Ok(e) = events.try_recv() {
        assert!(!format!("{e:?}").contains(SECRET), "event leaked: {e:?}");
    }
}

#[tokio::test]
async fn exports_refuse_stale_snapshots_existing_files_and_missing_folders() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = seeded(dir.path()).await;
    let first = core
        .diagnostics_preview(DiagnosticsPreviewParams::default())
        .await
        .unwrap();
    let second = core
        .diagnostics_preview(DiagnosticsPreviewParams::default())
        .await
        .unwrap();
    let export = |id: &str, path: std::path::PathBuf, replace: bool| DiagnosticsExportParams {
        snapshot_id: id.to_owned(),
        path: path.display().to_string(),
        replace,
    };
    let out = dir.path().join("d.json");
    let e = core
        .diagnostics_export(export(&first.snapshot_id, out.clone(), false))
        .await
        .unwrap_err();
    assert_eq!(
        e.0.kind,
        ErrorKind::InvalidRequest,
        "only the latest preview exports"
    );
    std::fs::write(&out, b"user's own file").unwrap();
    let e = core
        .diagnostics_export(export(&second.snapshot_id, out.clone(), false))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::DestinationUnwritable);
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"user's own file",
        "never replaced unasked"
    );
    core.diagnostics_export(export(&second.snapshot_id, out.clone(), true))
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), second.content);
    let e = core
        .diagnostics_export(export(
            &second.snapshot_id,
            dir.path().join("no").join("d.json"),
            false,
        ))
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::DestinationUnwritable);
    assert!(second.content.len() <= crate::core::diagnostics::MAX_BYTES);
}

#[tokio::test]
async fn the_report_stays_bounded_with_many_jobs_and_failures() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    )
    .with_max_active(16);
    for i in 0..300 {
        let s = core
            .add(add_params(
                &format!("diag-bound-{i:05}"),
                &format!("http://h/{i}.bin"),
            ))
            .await
            .unwrap();
        if i % 2 == 0 && s.attempt_id.is_some() {
            core.backend().set_progress(
                &handle_of(&s),
                progress(
                    TransferState::Failed(BackendError::new(ErrorKind::InvalidRange, "x")),
                    0,
                    None,
                ),
            );
        }
    }
    core.poll_once().await;
    let p = core
        .diagnostics_preview(DiagnosticsPreviewParams::default())
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&p.content).unwrap();
    assert_eq!(v["jobs"]["total"], 300);
    assert_eq!(v["jobs"]["summarized"], 200);
    assert!(v["recent_failures"].as_array().unwrap().len() <= 50);
    assert!(p.content.len() <= crate::core::diagnostics::MAX_BYTES);
}
