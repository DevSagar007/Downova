//! CORE-08 in the service (ADR 0025), with a controlled wall clock.
use super::*;
use dm_domain::api::SetQueueScheduleParams;
use std::sync::Mutex as StdMutex;

/// A clock the test moves.
#[derive(Clone)]
struct TestClock(Arc<StdMutex<Timestamp>>);
impl TestClock {
    fn at(local: &str) -> Self {
        Self(Arc::new(StdMutex::new(dhaka(local))))
    }
    fn set(&self, local: &str) {
        *self.0.lock().unwrap() = dhaka(local);
    }
    fn f(&self) -> Arc<dyn Fn() -> Timestamp + Send + Sync> {
        let c = Arc::clone(&self.0);
        Arc::new(move || *c.lock().unwrap())
    }
}
fn dhaka(local: &str) -> Timestamp {
    local
        .parse::<jiff::civil::DateTime>()
        .unwrap()
        .to_zoned(jiff::tz::TimeZone::get("Asia/Dhaka").unwrap())
        .unwrap()
        .timestamp()
}
/// Mondays 01:00–07:00 in Dhaka (2026-11-02 is a Monday).
fn monday_night(missed: &str) -> QueueSchedule {
    serde_json::from_str(&format!(
        r#"{{"enabled":true,"timezone":"Asia/Dhaka","windows":[{{"days":["mon"],"start":"01:00","end":"07:00"}}],"missed_window":"{missed}"}}"#
    ))
    .unwrap()
}
fn timed(dir: &Path, clock: &TestClock) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir).with_clock(clock.f())
}
fn timed_store(db: &Path, dir: &Path, clock: &TestClock) -> Core<FakeBackend> {
    timed(dir, clock)
        .with_store(Store::open(db).unwrap())
        .unwrap()
}
async fn schedule(core: &Core<FakeBackend>, s: Option<QueueSchedule>) -> QueueInfo {
    core.set_queue_schedule(SetQueueScheduleParams {
        queue_id: "main".into(),
        schedule: s,
    })
    .await
    .unwrap()
}
fn starts(core: &Core<FakeBackend>) -> usize {
    core.backend()
        .calls()
        .iter()
        .filter(|c| c.starts_with("start"))
        .count()
}

#[tokio::test]
async fn outside_its_window_a_job_waits_and_the_window_opening_admits_it() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-11-02T00:00:00");
    let core = timed(dir.path(), &clock);
    let info = schedule(&core, Some(monday_night("skip"))).await;
    let state = info.schedule_state.unwrap();
    assert!(!state.in_window);
    assert_eq!(
        state.next_change_at.as_deref(),
        Some("2026-11-01T19:00:00Z")
    );
    let s = core
        .add(add_params("sched-wait-0001", "http://h/a.bin"))
        .await
        .unwrap();
    assert!(s.blockers.contains(&Blocker::ScheduleWindow));
    assert_eq!(starts(&core), 0, "nothing starts outside the window");
    clock.set("2026-11-02T01:00:00");
    core.poll_once().await;
    let s = core.get(&s.job_id).await.unwrap();
    assert!(!s.blockers.contains(&Blocker::ScheduleWindow));
    assert_eq!(starts(&core), 1, "the window opening admitted it");
}

#[tokio::test]
async fn a_window_never_resumes_a_manual_pause_or_a_stopped_queue() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-11-02T00:00:00");
    let core = timed(dir.path(), &clock);
    schedule(&core, Some(monday_night("skip"))).await;
    let paused = core
        .add(add_params("sched-pause-001", "http://h/p.bin"))
        .await
        .unwrap();
    core.pause(&paused.job_id).await.unwrap();
    clock.set("2026-11-02T01:30:00");
    core.poll_once().await;
    let p = core.get(&paused.job_id).await.unwrap();
    assert_eq!(p.desired_state, DesiredState::Paused, "still paused");
    assert_eq!(starts(&core), 0);
    // A stopped queue stays stopped inside its window.
    core.set_queue_running("main", false).await.unwrap();
    core.resume(&paused.job_id).await.unwrap();
    core.poll_once().await;
    assert_eq!(starts(&core), 0, "the stopped queue holds it");
    core.set_queue_running("main", true).await.unwrap();
    assert_eq!(
        starts(&core),
        1,
        "starting the queue inside the window admits it"
    );
}

#[tokio::test]
async fn a_closing_window_holds_a_checkpointed_transfer_and_lets_one_without_a_checkpoint_finish() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-11-02T02:00:00");
    let core = timed(dir.path(), &clock);
    core.set_queue_limit("main", 4).await.unwrap();
    schedule(&core, Some(monday_night("skip"))).await;
    let resumable = core
        .add(add_params("sched-res-00001", "http://h/r.bin"))
        .await
        .unwrap();
    let streaming = core
        .add(add_params("sched-str-00001", "http://h/s.bin"))
        .await
        .unwrap();
    assert_eq!(starts(&core), 2);
    // Only the first has a durable checkpoint.
    {
        let mut state = core.state.lock().await;
        let job = state.job_mut(&resumable.job_id).unwrap();
        job.durable = 4096;
    }
    clock.set("2026-11-02T07:00:00");
    core.poll_once().await;
    let r = core.get(&resumable.job_id).await.unwrap();
    let s = core.get(&streaming.job_id).await.unwrap();
    assert!(
        r.blockers.contains(&Blocker::ScheduleWindow),
        "held at the window's end"
    );
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c == &format!("pause {}", r.attempt_id.clone().unwrap().as_str())),
        "the checkpointed transfer was paused"
    );
    assert!(
        !s.blockers.contains(&Blocker::ScheduleWindow),
        "no checkpoint: it runs on"
    );
    assert_eq!(s.desired_state, DesiredState::Running);
}

#[tokio::test]
async fn a_restart_outside_the_window_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let clock = TestClock::at("2026-11-02T08:00:00");
    let job = {
        let core = timed_store(&db, dir.path(), &clock);
        schedule(&core, Some(monday_night("skip"))).await;
        core.add(add_params("sched-restart-01", "http://h/x.bin"))
            .await
            .unwrap()
            .job_id
    };
    clock.set("2026-11-03T12:00:00");
    let core = timed_store(&db, dir.path(), &clock);
    core.recover().await;
    core.poll_once().await;
    assert_eq!(starts(&core), 0);
    assert!(
        core.get(&job)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::ScheduleWindow)
    );
    let q = core
        .queues()
        .await
        .into_iter()
        .find(|q| q.queue_id == "main")
        .unwrap();
    assert_eq!(
        q.schedule,
        Some(monday_night("skip")),
        "the schedule is durable"
    );
}

#[tokio::test]
async fn a_missed_window_is_caught_up_once_bounded_and_never_extended_by_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let clock = TestClock::at("2026-11-02T00:00:00");
    {
        let core = timed_store(&db, dir.path(), &clock);
        schedule(&core, Some(monday_night("catch_up"))).await;
        core.add(add_params("sched-catch-0001", "http://h/c.bin"))
            .await
            .unwrap();
    }
    // The computer was off through the whole window (01:00–07:00).
    clock.set("2026-11-02T09:00:00");
    let core = timed_store(&db, dir.path(), &clock);
    let q = core
        .queues()
        .await
        .into_iter()
        .find(|q| q.queue_id == "main")
        .unwrap();
    let state = q.schedule_state.unwrap();
    assert!(state.in_window, "a catch-up window is open");
    // Six hours (the window's length, capped at 6 h) from 09:00.
    assert_eq!(
        state.catch_up_until.as_deref(),
        Some("2026-11-02T09:00:00Z")
    );
    core.recover().await;
    core.poll_once().await;
    assert_eq!(
        starts(&core),
        1,
        "the waiting job runs in the catch-up window"
    );
    drop(core);
    // A restart during the catch-up keeps the same window: not extended, not repeated.
    clock.set("2026-11-02T10:00:00");
    let core = timed_store(&db, dir.path(), &clock);
    let q = core
        .queues()
        .await
        .into_iter()
        .find(|q| q.queue_id == "main")
        .unwrap();
    assert_eq!(
        q.schedule_state.unwrap().catch_up_until.as_deref(),
        Some("2026-11-02T09:00:00Z")
    );
    drop(core);
    // After it ended, the same missed window is never caught up again.
    clock.set("2026-11-02T16:00:00");
    let core = timed_store(&db, dir.path(), &clock);
    let q = core
        .queues()
        .await
        .into_iter()
        .find(|q| q.queue_id == "main")
        .unwrap();
    let state = q.schedule_state.unwrap();
    assert!(!state.in_window);
    assert_eq!(state.catch_up_until, None);
}

#[tokio::test]
async fn an_unknown_zone_or_a_refused_save_changes_nothing_and_removal_releases_the_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let clock = TestClock::at("2026-11-02T00:00:00");
    let mut core = timed_store(&db, dir.path(), &clock);
    let mut bad = monday_night("skip");
    bad.timezone = "Mars/Olympus".into();
    let e = core
        .set_queue_schedule(SetQueueScheduleParams {
            queue_id: "main".into(),
            schedule: Some(bad),
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::InvalidRequest);
    let job = core
        .add(add_params("sched-refuse-001", "http://h/f.bin"))
        .await
        .unwrap();
    assert!(job.blockers.is_empty(), "no schedule yet");
    // The database refuses further queue changes (set while the service is closed).
    drop(core);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_schedule BEFORE UPDATE ON queues BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    core = timed_store(&db, dir.path(), &clock);
    let e = core
        .set_queue_schedule(SetQueueScheduleParams {
            queue_id: "main".into(),
            schedule: Some(monday_night("skip")),
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::Internal);
    assert!(
        core.get(&job.job_id).await.unwrap().blockers.is_empty(),
        "no blocker applied"
    );
    let q = core
        .queues()
        .await
        .into_iter()
        .find(|q| q.queue_id == "main")
        .unwrap();
    assert_eq!(q.schedule, None);
    drop(core);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_schedule;")
        .unwrap();
    let core = timed_store(&db, dir.path(), &clock);
    core.recover().await;
    // Set, then remove: the hold comes and goes.
    let held = core
        .add(add_params("sched-refuse-002", "http://h/g.bin"))
        .await
        .unwrap();
    core.pause(&held.job_id).await.unwrap();
    schedule(&core, Some(monday_night("skip"))).await;
    assert!(
        core.get(&held.job_id)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::ScheduleWindow)
    );
    schedule(&core, None).await;
    assert!(
        !core
            .get(&held.job_id)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::ScheduleWindow)
    );
}

#[tokio::test]
async fn a_clock_jump_is_applied_at_the_next_poll() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-11-02T00:00:00");
    let core = timed(dir.path(), &clock);
    schedule(&core, Some(monday_night("skip"))).await;
    let s = core
        .add(add_params("sched-jump-0001", "http://h/j.bin"))
        .await
        .unwrap();
    // Forward into the window: the passed edge is applied at once.
    clock.set("2026-11-02T03:00:00");
    core.poll_once().await;
    assert_eq!(starts(&core), 1);
    // Back before the window: re-evaluated at the next poll (the clock moved back), held again
    // only once the transfer has a checkpoint; waiting jobs are held at once.
    clock.set("2026-11-01T23:00:00");
    let w = core
        .add(add_params("sched-jump-0002", "http://h/k.bin"))
        .await
        .unwrap();
    core.poll_once().await;
    assert!(
        core.get(&w.job_id)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::ScheduleWindow)
    );
    let _ = s;
}

#[tokio::test]
async fn a_moved_job_takes_the_destination_queues_schedule_and_deleting_it_releases_them() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-11-02T00:00:00");
    let core = timed(dir.path(), &clock);
    let night = core.create_queue("Night", None).await.unwrap();
    core.set_queue_schedule(SetQueueScheduleParams {
        queue_id: night.queue_id.clone(),
        schedule: Some(monday_night("skip")),
    })
    .await
    .unwrap();
    let job = core
        .add(add_params("sched-move-0001", "http://h/m.bin"))
        .await
        .unwrap();
    core.pause(&job.job_id).await.unwrap();
    let moved = core
        .set_job_queue(&job.job_id, &night.queue_id)
        .await
        .unwrap();
    assert!(
        moved.blockers.contains(&Blocker::ScheduleWindow),
        "the destination's hold"
    );
    let back = core.set_job_queue(&job.job_id, "main").await.unwrap();
    assert!(
        !back.blockers.contains(&Blocker::ScheduleWindow),
        "main has no schedule"
    );
    core.set_job_queue(&job.job_id, &night.queue_id)
        .await
        .unwrap();
    core.delete_queue(&night.queue_id, Some("main"))
        .await
        .unwrap();
    let s = core.get(&job.job_id).await.unwrap();
    assert!(
        !s.blockers.contains(&Blocker::ScheduleWindow),
        "deleting the queue releases it"
    );
    assert_eq!(
        s.desired_state,
        DesiredState::Paused,
        "the manual pause is kept"
    );
}
