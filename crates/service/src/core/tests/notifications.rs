//! CORE-09 (ADR 0022) against the fake backend and a recording notifier: only durable terminal
//! outcomes notify, once each; settings, cancellation, Retry, restarts and startup behave as
//! documented; the notifier runs outside the coordinator lock.

use std::sync::Arc;

use super::*;
use crate::notify::recording::RecordingNotifier;
use dm_domain::api::{NotificationLocale, NotificationSettings, RetryParams};

fn with_notifier(dir: &Path, notifier: Arc<RecordingNotifier>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir).with_notifier(notifier)
}

async fn on(core: &Core<FakeBackend>, locale: NotificationLocale) {
    core.set_notifications(NotificationSettings {
        enabled: true,
        locale,
    })
    .await
    .unwrap();
}

/// Waits until the blocking hand-offs ran (they are spawned after the lock is released).
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

fn titles(n: &RecordingNotifier) -> Vec<String> {
    n.shown
        .lock()
        .unwrap()
        .iter()
        .map(|x| x.title.clone())
        .collect()
}

/// Drives a job to verified + placed completion.
async fn complete(core: &Core<FakeBackend>, dir: &Path, job: &DownloadSnapshot) {
    std::fs::write(staged(dir, job), b"done").unwrap();
    core.backend().set_progress(
        &handle_of(job),
        progress(TransferState::Complete, 4, Some(4)),
    );
    core.poll_once().await;
}

#[tokio::test]
async fn completion_notifies_once_only_after_the_durable_completed_state() {
    let dir = tempfile::tempdir().unwrap();
    let n = Arc::new(RecordingNotifier::default());
    let core = with_notifier(dir.path(), n.clone());
    on(&core, NotificationLocale::En).await;
    settle().await;
    assert_eq!(
        titles(&n),
        ["Notifications are on"],
        "the explicit enabling action"
    );
    let job = core
        .add(add_params("ext_nt000001", "http://h/report.pdf"))
        .await
        .unwrap();
    // Accepted, transferring and 100 % transferred are not completion.
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Active, 4, Some(4)),
    );
    core.poll_once().await;
    settle().await;
    assert_eq!(titles(&n).len(), 1, "nothing before completion");
    complete(&core, dir.path(), &job).await;
    settle().await;
    let shown = n.shown.lock().unwrap().clone();
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[1].title, "Download complete");
    assert!(shown[1].body.starts_with("report.pdf"));
    assert!(!shown[1].body.contains("http"), "never the link");
    // Later polls (and event resubscriptions, which never reach the notifier) add nothing.
    core.poll_once().await;
    core.poll_once().await;
    settle().await;
    assert_eq!(titles(&n).len(), 2);
}

#[tokio::test]
async fn failures_notify_cancellations_and_disabled_outcomes_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let n = Arc::new(RecordingNotifier::default());
    let core = with_notifier(dir.path(), n.clone());
    // Off (the default): a failure is decided as skipped and never notified later.
    let early = core
        .add(add_params("ext_nt000002", "http://h/early.bin"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&early),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::RemoteChanged, "x")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    on(&core, NotificationLocale::Bn).await;
    core.poll_once().await;
    settle().await;
    assert_eq!(
        titles(&n),
        ["নোটিফিকেশন চালু"],
        "no catch-up for the earlier failure"
    );
    // On: a failure notifies (in Bangla); a user cancellation does not.
    let failing = core
        .add(add_params("ext_nt000003", "http://h/broken.iso"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&failing),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::RemoteChanged, "x")),
            0,
            None,
        ),
    );
    let cancelled = core
        .add(add_params("ext_nt000004", "http://h/stop.bin"))
        .await
        .unwrap();
    core.cancel(&cancelled.job_id).await.unwrap();
    core.poll_once().await;
    settle().await;
    assert_eq!(titles(&n), ["নোটিফিকেশন চালু", "ডাউনলোড ব্যর্থ"]);
    // An automatic retry (transient error) is not terminal: no notification.
    let transient = core
        .add(add_params("ext_nt000005", "http://h/flaky.bin"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&transient),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::NetworkOffline, "x")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    settle().await;
    assert_eq!(titles(&n).len(), 2);
}

#[tokio::test]
async fn a_retried_job_notifies_its_new_outcome_and_a_blocked_system_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let n = Arc::new(RecordingNotifier::default());
    let core = with_notifier(dir.path(), n.clone());
    on(&core, NotificationLocale::En).await;
    let job = core
        .add(add_params("ext_nt000006", "http://h/again.bin"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::RemoteChanged, "x")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    let retried = core
        .retry(RetryParams {
            job_id: job.job_id.clone(),
            operation_id: "op_nt_retry01".into(),
            expected_attempt_id: None,
            discard_partial: false,
        })
        .await
        .unwrap();
    // A replay of the same Retry changes nothing and notifies nothing.
    core.retry(RetryParams {
        job_id: job.job_id.clone(),
        operation_id: "op_nt_retry01".into(),
        expected_attempt_id: None,
        discard_partial: false,
    })
    .await
    .unwrap();
    n.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    complete(&core, dir.path(), &retried).await;
    settle().await;
    assert_eq!(
        titles(&n),
        [
            "Notifications are on",
            "Download failed",
            "Download complete"
        ]
    );
    // The system refused the last one: reported, never retried, downloads unaffected.
    let status = core.notification_status().await;
    assert!(status.last_delivery_failed);
    assert_eq!(core.get(&job.job_id).await.unwrap().phase, Phase::Completed);
    core.poll_once().await;
    settle().await;
    assert_eq!(titles(&n).len(), 3);
}

#[tokio::test]
async fn a_restart_never_repeats_or_backfills_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let n = Arc::new(RecordingNotifier::default());
    let job_id;
    {
        let core = with_notifier(dir.path(), n.clone())
            .with_store(Store::open(&db).unwrap())
            .unwrap();
        on(&core, NotificationLocale::En).await;
        let job = core
            .add(add_params("ext_nt000007", "http://h/kept.bin"))
            .await
            .unwrap();
        job_id = job.job_id.clone();
        complete(&core, dir.path(), &job).await;
        settle().await;
    }
    assert_eq!(titles(&n), ["Notifications are on", "Download complete"]);
    let again = Arc::new(RecordingNotifier::default());
    let core = with_notifier(dir.path(), again.clone())
        .with_store(Store::open(&db).unwrap())
        .unwrap();
    assert!(
        core.notification_status().await.enabled,
        "the setting persists"
    );
    core.poll_once().await;
    core.poll_once().await;
    settle().await;
    assert!(again.shown.lock().unwrap().is_empty(), "nothing at startup");
    assert_eq!(core.get(&job_id).await.unwrap().phase, Phase::Completed);
}

#[tokio::test]
async fn enabling_without_a_system_adapter_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    );
    let e = core
        .set_notifications(NotificationSettings {
            enabled: true,
            locale: NotificationLocale::En,
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::BackendCapabilityUnavailable);
    let status = core.notification_status().await;
    assert!(!status.enabled);
    assert_eq!(
        status.availability,
        dm_domain::api::NotificationAvailability::Unavailable
    );
}
