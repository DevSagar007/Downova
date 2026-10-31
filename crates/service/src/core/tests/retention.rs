//! B2.12 history retention (ADR 0018) against the fake backend: eligibility, boundaries, clock
//! changes, bounded batches, events, idempotent replays and persistence across a restart.

use super::*;
use dm_domain::api::EventKind;
use dm_domain::api::HistorySettings;

const DAY: u64 = 24 * 60 * 60 * 1000;

fn supported(dir: &Path) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
}

/// A job that completed through the journal, with its file on disk.
async fn completed(core: &Core<FakeBackend>, dir: &Path, key: &str) -> DownloadSnapshot {
    let job = core
        .add(add_params(
            &format!("ext_{key}"),
            &format!("http://h/{key}.bin"),
        ))
        .await
        .unwrap();
    std::fs::write(staged(dir, &job), key.as_bytes()).unwrap();
    let len = key.len() as u64;
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, len, Some(len)),
    );
    core.poll_once().await;
    let done = core.get(&job.job_id).await.unwrap();
    assert_eq!(done.phase, Phase::Completed, "{:?}", done.error);
    done
}

/// Sets when a job finished (tests control time; the service uses the stored stamp).
async fn finished_at(core: &Core<FakeBackend>, job: &DownloadSnapshot, at: u64) {
    let mut state = core.state.lock().await;
    state.job_mut(&job.job_id).unwrap().finished_at_ms = Some(at);
}

async fn listed(core: &Core<FakeBackend>) -> Vec<String> {
    core.list()
        .await
        .into_iter()
        .map(|d| d.job_id.as_str().to_owned())
        .collect()
}

async fn keep(core: &Core<FakeBackend>, days: Option<u32>) {
    core.set_history(HistorySettings {
        retention_days: days,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn keeping_forever_is_the_default_and_never_removes_anything() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    assert_eq!(core.history_settings().await.retention_days, None);
    let job = completed(&core, dir.path(), "kf-0001").await;
    finished_at(&core, &job, 0).await;
    let swept = core.sweep_history_at(1_000 * DAY, 100).await;
    assert_eq!((swept.removed, swept.kept), (0, 0));
    assert_eq!(listed(&core).await, [job.job_id.as_str()]);
}

#[tokio::test]
async fn a_job_is_due_exactly_when_the_period_has_passed() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(7)).await;
    let job = completed(&core, dir.path(), "bd-0001").await;
    finished_at(&core, &job, 1_000).await;
    let swept = core.sweep_history_at(1_000 + 7 * DAY - 1, 100).await;
    assert_eq!(swept.removed, 0, "one millisecond early");
    let swept = core.sweep_history_at(1_000 + 7 * DAY, 100).await;
    assert_eq!(swept.removed, 1);
    let gone = core.get(&job.job_id).await.unwrap();
    assert!(gone.removed, "a tombstone: still answers get");
    // The file is kept byte for byte.
    let file = dir.path().join(&job.display_name);
    assert_eq!(std::fs::read(file).unwrap(), b"bd-0001");
}

#[tokio::test]
async fn unfinished_paused_queued_and_finalizing_work_never_expires() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(1)).await;
    let running = core
        .add(add_params("ext_un-0001", "http://h/a.bin"))
        .await
        .unwrap();
    let paused = core
        .add(add_params("ext_un-0002", "http://h/b.bin"))
        .await
        .unwrap();
    core.pause(&paused.job_id).await.unwrap();
    // Two more wait for a slot (queue_limit) behind the active ones.
    let queued = core
        .add(add_params("ext_un-0003", "http://h/c.bin"))
        .await
        .unwrap();
    // A completed job still finalizing (its journal entry is open) is not removable.
    let finalizing = completed(&core, dir.path(), "un-0004").await;
    {
        let mut state = core.state.lock().await;
        for job in state.jobs.iter_mut() {
            job.finished_at_ms = Some(0);
        }
        let f = state.job_mut(&finalizing.job_id).unwrap();
        f.finalizing = Some(dm_storage::JournalRow {
            job_id: f.id.clone(),
            attempt_id: AttemptId::parse("att_00000000000000aa").unwrap(),
            stage: dm_storage::JournalStage::Complete,
            placement: dm_storage::Placement::Staged,
            staging_path: dir.path().join("x.downova-part"),
            final_path: dir.path().join("x"),
            length: None,
            sha256: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        });
    }
    let swept = core.sweep_history_at(1_000 * DAY, 100).await;
    assert_eq!(swept.removed, 0);
    let ids = listed(&core).await;
    for job in [&running, &paused, &queued, &finalizing] {
        assert!(
            ids.contains(&job.job_id.as_str().to_owned()),
            "{}",
            job.job_id
        );
    }
}

#[tokio::test]
async fn failed_and_cancelled_jobs_expire_like_completed_ones() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(30)).await;
    let failed = core
        .add(add_params("ext_fc-0001", "http://h/f.bin"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&failed),
        progress(
            TransferState::Failed(BackendError::new(ErrorKind::LinkRefreshRequired, "410")),
            0,
            None,
        ),
    );
    core.poll_once().await;
    assert_eq!(core.get(&failed.job_id).await.unwrap().phase, Phase::Failed);
    let cancelled = core
        .add(add_params("ext_fc-0002", "http://h/c.bin"))
        .await
        .unwrap();
    core.cancel(&cancelled.job_id).await.unwrap();
    // Finished in this run: the first sweep stamps "now", so nothing is due yet.
    let now = 50 * DAY;
    assert_eq!(core.sweep_history_at(now, 100).await.removed, 0);
    assert_eq!(core.sweep_history_at(now + 30 * DAY, 100).await.removed, 2);
    assert!(listed(&core).await.is_empty());
}

#[tokio::test]
async fn a_clock_that_went_back_never_makes_anything_due() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(1)).await;
    let job = completed(&core, dir.path(), "cb-0001").await;
    finished_at(&core, &job, 100 * DAY).await;
    // "Now" is before the finish time (the clock was set back).
    assert_eq!(core.sweep_history_at(10 * DAY, 100).await.removed, 0);
    assert_eq!(listed(&core).await.len(), 1);
}

#[tokio::test]
async fn sweeps_are_bounded_and_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(1)).await;
    let mut jobs = Vec::new();
    for i in 0..5 {
        let job = core
            .add(add_params(
                &format!("ext_bt-{i:04}"),
                &format!("http://h/{i}.bin"),
            ))
            .await
            .unwrap();
        core.cancel(&job.job_id).await.unwrap();
        finished_at(&core, &job, (10 - i) as u64).await; // job 4 finished first
        jobs.push(job);
    }
    let swept = core.sweep_history_at(100 * DAY, 2).await;
    assert_eq!(swept.removed, 2);
    let left = listed(&core).await;
    assert!(
        !left.contains(&jobs[4].job_id.as_str().to_owned()),
        "oldest went first"
    );
    assert!(!left.contains(&jobs[3].job_id.as_str().to_owned()));
    assert_eq!(left.len(), 3);
    assert_eq!(core.sweep_history_at(100 * DAY, 100).await.removed, 3);
}

#[tokio::test]
async fn preview_counts_exactly_what_a_policy_would_remove_without_changing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let old = completed(&core, dir.path(), "pv-0001").await;
    let new = completed(&core, dir.path(), "pv-0002").await;
    let _running = core
        .add(add_params("ext_pv-0003", "http://h/r.bin"))
        .await
        .unwrap();
    let now = now_ms();
    finished_at(&core, &old, now.saturating_sub(40 * DAY)).await;
    finished_at(&core, &new, now.saturating_sub(DAY)).await;
    let p = core.preview_history(Some(30)).await.unwrap();
    assert_eq!((p.expiring, p.finished), (1, 2));
    let p = core.preview_history(Some(365)).await.unwrap();
    assert_eq!((p.expiring, p.finished), (0, 2));
    let p = core.preview_history(None).await.unwrap();
    assert_eq!((p.expiring, p.finished), (0, 2));
    assert_eq!(listed(&core).await.len(), 3, "a preview removes nothing");
    assert_eq!(
        core.history_settings().await.retention_days,
        None,
        "and saves nothing"
    );
    for bad in [Some(0), Some(3651)] {
        let err = core.preview_history(bad).await.unwrap_err();
        assert_eq!(err.0.kind, ErrorKind::InvalidRequest);
        let err = core
            .set_history(HistorySettings {
                retention_days: bad,
            })
            .await
            .unwrap_err();
        assert_eq!(err.0.kind, ErrorKind::InvalidRequest);
    }
}

#[tokio::test]
async fn retention_publishes_removal_events_and_replays_still_answer() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    keep(&core, Some(1)).await;
    let job = completed(&core, dir.path(), "ev-0001").await;
    finished_at(&core, &job, 0).await;
    let mut sub = core.subscribe().await;
    assert_eq!(sub.start.items.len(), 1);
    assert_eq!(core.sweep_history_at(10 * DAY, 100).await.removed, 1);
    let event = sub.events.try_recv().expect("a removal event");
    assert_eq!(event.kind, EventKind::JobRemoved);
    assert_eq!(event.job_id.as_ref(), Some(&job.job_id));
    // The same key replays the original (removed) job; nothing new is created.
    let again = core
        .add(add_params("ext_ev-0001", "http://h/ev-0001.bin"))
        .await
        .unwrap();
    assert_eq!(again.job_id, job.job_id);
    assert!(again.removed);
    assert!(listed(&core).await.is_empty());
}

#[tokio::test]
async fn the_policy_and_finish_times_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let job = {
        let core = stored_core(&db, dir.path());
        keep(&core, Some(30)).await;
        completed(&core, dir.path(), "rs-0001").await
    };
    let core = stored_core(&db, dir.path());
    assert_eq!(core.history_settings().await.retention_days, Some(30));
    let stamp = {
        let mut state = core.state.lock().await;
        state.job_mut(&job.job_id).unwrap().finished_at_ms
    };
    let stamp = stamp.expect("the stored finish time");
    assert_eq!(
        core.sweep_history_at(stamp + 30 * DAY - 1, 100)
            .await
            .removed,
        0
    );
    assert_eq!(
        core.sweep_history_at(stamp + 30 * DAY, 100).await.removed,
        1
    );
    drop(core);
    // The removal is durable.
    let core = stored_core(&db, dir.path());
    assert!(listed(&core).await.is_empty());
    assert!(core.get(&job.job_id).await.unwrap().removed);
}
