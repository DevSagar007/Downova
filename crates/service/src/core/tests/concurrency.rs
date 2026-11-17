use super::*;
use dm_domain::api::{ConcurrencySettings, ServiceSettings};

fn supported(dir: &Path) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
}

async fn active(core: &Core<FakeBackend>) -> usize {
    core.state
        .lock()
        .await
        .jobs
        .iter()
        .filter(|j| j.is_active())
        .count()
}

type Routed = crate::router::EngineRouter<FakeBackend, FakeBackend>;

/// Both engines, every capability supported (ADR 0043).
fn routed(dir: &Path) -> Core<Routed> {
    let mut profiles = Profiles::new();
    profiles.insert("downloads".into(), dir.to_owned());
    let router = crate::router::EngineRouter::new(
        FakeBackend::with_all(CapabilitySupport::Supported),
        Some((FakeBackend::with_all(CapabilitySupport::Supported), None)),
    );
    Core::new(router, Some("test".into()), profiles).with_scanner(Arc::new(attachment::NoScanner))
}

fn media_params(key: &str, url: &str) -> AddDownloadParams {
    AddDownloadParams {
        engine: Some(Engine::Media),
        ..add_params(key, url)
    }
}

async fn active_by_engine(core: &Core<Routed>) -> (usize, usize) {
    let state = core.state.lock().await;
    let count = |e: Engine| {
        state
            .jobs
            .iter()
            .filter(|j| j.engine == e && j.is_active())
            .count()
    };
    (count(Engine::File), count(Engine::Media))
}

/// ADR 0043: at most three media downloads run at once, and media waiting for its own cap never
/// holds back an ordinary file; each attempt goes to its own engine.
#[tokio::test]
async fn media_has_its_own_cap_and_never_blocks_ordinary_files() {
    let dir = tempfile::tempdir().unwrap();
    let core = routed(dir.path());
    core.set_queue_limit("main", 16).await.unwrap();
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: 8,
    })
    .await
    .unwrap();
    for i in 0..5 {
        let snap = core
            .add(media_params(
                &format!("media-cap-{i:04}"),
                &format!("https://h/watch{i}"),
            ))
            .await
            .unwrap();
        assert_eq!(snap.engine, Engine::Media);
    }
    for i in 0..4 {
        let snap = core
            .add(add_params(
                &format!("file-next-{i:04}"),
                &format!("http://h/{i}.zip"),
            ))
            .await
            .unwrap();
        assert_eq!(snap.engine, Engine::File);
    }
    // 8 slots: 3 media (their cap) and all 4 files; the other 2 media wait.
    assert_eq!(active_by_engine(&core).await, (4, 3));
    let backend = core.backend();
    assert_eq!(backend.media().unwrap().started.lock().unwrap().len(), 3);
    assert_eq!(backend.file().started.lock().unwrap().len(), 4);
    assert!(
        backend
            .media()
            .unwrap()
            .started
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.engine == Engine::Media)
    );
    // A waiting media job says why: it waits for a slot.
    let list = core.list().await;
    let waiting: Vec<_> = list
        .iter()
        .filter(|s| s.engine == Engine::Media && s.attempt_id.is_none())
        .collect();
    assert_eq!(waiting.len(), 2);
    assert!(
        waiting
            .iter()
            .all(|s| s.blockers.contains(&Blocker::QueueLimit))
    );
}

/// ADR 0043: with no media engine installed, a media add is refused before anything is created,
/// and the same key works once it is sent as an ordinary file.
#[tokio::test]
async fn a_media_add_without_a_media_engine_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let err = core
        .add(media_params("media-none-0001", "https://h/watch"))
        .await
        .unwrap_err();
    assert_eq!(err.0.kind, ErrorKind::BackendCapabilityUnavailable);
    assert!(core.list().await.is_empty());
    assert!(core.backend().started.lock().unwrap().is_empty());
    let file = core
        .add(add_params("media-none-0001", "https://h/watch"))
        .await
        .unwrap();
    assert_eq!(file.engine, Engine::File);
}

/// The media engine's own report decides: an FTP link or a per-download speed limit it does not
/// support is refused for media even though the native engine supports both.
#[tokio::test]
async fn media_adds_are_checked_against_the_media_engines_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let mut profiles = Profiles::new();
    profiles.insert("downloads".into(), dir.path().to_owned());
    let mut media = FakeBackend::with_all(CapabilitySupport::Unsupported);
    for c in &mut media.caps {
        if matches!(c.capability, Capability::Http | Capability::Https) {
            c.support = CapabilitySupport::Supported;
        }
    }
    let core = Core::new(
        crate::router::EngineRouter::new(
            FakeBackend::with_all(CapabilitySupport::Supported),
            Some((media, None)),
        ),
        None,
        profiles,
    );
    let ftp = core
        .add(media_params("media-caps-0001", "ftp://h/a.mp4"))
        .await
        .unwrap_err();
    assert_eq!(ftp.0.kind, ErrorKind::BackendCapabilityUnavailable);
    let limited = core
        .add(AddDownloadParams {
            speed_limit_bytes_per_second: Some(ByteCount(64 * 1024)),
            ..media_params("media-caps-0002", "https://h/watch")
        })
        .await
        .unwrap_err();
    assert_eq!(limited.0.kind, ErrorKind::BackendCapabilityUnavailable);
    // The same requests as ordinary files are fine.
    core.add(add_params("media-caps-0003", "ftp://h/a.mp4"))
        .await
        .unwrap();
    let hello = core.hello();
    assert!(hello.media.is_some());
    assert_eq!(hello.backend.id, "fake");
}

#[tokio::test]
async fn raising_the_global_limit_admits_more_than_three_and_lowering_never_preempts() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    assert_eq!(core.concurrency_settings().await.max_active_downloads, 3);
    core.set_queue_limit("main", 16).await.unwrap();
    let mut jobs = Vec::new();
    for i in 0..8 {
        jobs.push(
            core.add(add_params(
                &format!("concurrent-{i:04}"),
                &format!("http://h/{i}.bin"),
            ))
            .await
            .unwrap(),
        );
    }
    assert_eq!(active(&core).await, 3);
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: 6,
    })
    .await
    .unwrap();
    assert_eq!(active(&core).await, 6);
    let attempts: Vec<_> = core
        .list()
        .await
        .iter()
        .map(|j| j.attempt_id.clone())
        .collect();
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: 2,
    })
    .await
    .unwrap();
    assert_eq!(active(&core).await, 6, "lowering never preempts");
    assert_eq!(
        attempts,
        core.list()
            .await
            .iter()
            .map(|j| j.attempt_id.clone())
            .collect::<Vec<_>>()
    );
    for j in jobs.iter().take(5) {
        core.pause(&j.job_id).await.unwrap();
    }
    assert_eq!(
        active(&core).await,
        2,
        "one waiting job now takes the free slot"
    );
    for j in jobs.iter().take(5) {
        assert_eq!(
            core.get(&j.job_id).await.unwrap().desired_state,
            DesiredState::Paused
        );
    }
}

#[tokio::test]
async fn queue_limits_stopped_queues_and_cancellation_survive_a_global_increase() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_queue_limit("main", 1).await.unwrap();
    let other = core.create_queue("Stopped", Some(16)).await.unwrap();
    core.set_queue_running(&other.queue_id, false)
        .await
        .unwrap();
    let mut jobs = Vec::new();
    for i in 0..6 {
        jobs.push(
            core.add(add_params(
                &format!("limited-{i:04}"),
                &format!("http://h/{i}.bin"),
            ))
            .await
            .unwrap(),
        );
    }
    core.cancel(&jobs[5].job_id).await.unwrap();
    let stopped = core
        .add(AddDownloadParams {
            queue_id: Some(other.queue_id),
            ..add_params("stopped-0001", "http://h/stopped.bin")
        })
        .await
        .unwrap();
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: 16,
    })
    .await
    .unwrap();
    assert_eq!(active(&core).await, 1);
    assert!(
        core.get(&stopped.job_id)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::QueueStopped)
    );
    core.set_queue_limit("main", 6).await.unwrap();
    assert_eq!(active(&core).await, 5);
    assert_eq!(
        core.get(&jobs[5].job_id).await.unwrap().phase,
        Phase::Cancelled
    );
}

#[tokio::test]
async fn concurrency_is_saved_apart_from_old_settings_and_loaded_before_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrency.sqlite3");
    {
        let core = supported(dir.path())
            .with_store(Store::open(&path).unwrap())
            .unwrap();
        core.set_concurrency(ConcurrencySettings {
            max_active_downloads: 10,
        })
        .await
        .unwrap();
        core.set_settings(ServiceSettings::default()).await.unwrap();
    }
    let core = supported(dir.path())
        .with_store(Store::open(&path).unwrap())
        .unwrap();
    assert_eq!(core.concurrency_settings().await.max_active_downloads, 10);
    assert_eq!(
        core.queues().await[0].max_active,
        3,
        "a global edit never rewrites a queue"
    );
}

#[tokio::test]
async fn invalid_concurrency_never_changes_the_saved_or_effective_limit() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    for max_active_downloads in [0, 17, u32::MAX] {
        let e = core
            .set_concurrency(ConcurrencySettings {
                max_active_downloads,
            })
            .await
            .unwrap_err();
        assert_eq!(e.0.kind, ErrorKind::InvalidRequest);
    }
    for params in [
        "{}",
        "{\"max_active_downloads\":0}",
        "{\"max_active_downloads\":17}",
        "{\"max_active_downloads\":2.5}",
        "{\"max_active_downloads\":\"6\"}",
        "{\"max_active_downloads\":6,\"extra\":true}",
    ] {
        let line = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":\"limit\",\"method\":\"concurrency.v1.set\",\"params\":{params}}}"
        );
        let (reply, _) = crate::rpc::handle_line(&core, &line).await;
        assert!(reply.error.is_some(), "{params}");
    }
    assert_eq!(core.concurrency_settings().await.max_active_downloads, 3);
}

/// Drives an active job to verified, placed completion (the fake engine reports Complete).
async fn complete(core: &Core<FakeBackend>, dir: &Path, id: &JobId) {
    let job = core.get(id).await.unwrap();
    std::fs::write(staged(dir, &job), b"done").unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, 4, Some(4)),
    );
    core.poll_once().await;
}

async fn add_many(
    core: &Core<FakeBackend>,
    prefix: &str,
    n: usize,
    queue: Option<&str>,
) -> Vec<JobId> {
    let mut ids = Vec::new();
    for i in 0..n {
        let job = core
            .add(AddDownloadParams {
                queue_id: queue.map(str::to_owned),
                ..add_params(
                    &format!("{prefix}-{i:04}"),
                    &format!("http://h/{prefix}{i}.bin"),
                )
            })
            .await
            .unwrap();
        ids.push(job.job_id);
    }
    ids
}

async fn set_global(core: &Core<FakeBackend>, n: u32) {
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: n,
    })
    .await
    .unwrap();
}

/// B2.31: the smaller of the global and the queue limit applies, in both directions.
#[tokio::test]
async fn the_smaller_of_the_global_and_the_main_queue_limit_applies() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_queue_limit("main", 6).await.unwrap();
    set_global(&core, 1).await;
    add_many(&core, "min", 6, None).await;
    assert_eq!(active(&core).await, 1, "global 1, Main 6");
    set_global(&core, 6).await;
    assert_eq!(active(&core).await, 6, "global 6, Main 6");
    let dir2 = tempfile::tempdir().unwrap();
    let core = supported(dir2.path());
    core.set_queue_limit("main", 3).await.unwrap();
    set_global(&core, 6).await;
    add_many(&core, "min3", 6, None).await;
    assert_eq!(active(&core).await, 3, "global 6, Main 3");
    set_global(&core, 8).await;
    assert_eq!(
        active(&core).await,
        3,
        "global 8, Main 3: the queue still limits"
    );
}

/// B2.31: several queues together never exceed the global limit.
#[tokio::test]
async fn queues_together_respect_the_global_limit() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_queue_limit("main", 6).await.unwrap();
    let second = core.create_queue("Second", Some(6)).await.unwrap();
    set_global(&core, 4).await;
    add_many(&core, "queue-a", 4, None).await;
    add_many(&core, "queue-b", 4, Some(&second.queue_id)).await;
    assert_eq!(active(&core).await, 4);
    set_global(&core, 16).await;
    assert_eq!(
        active(&core).await,
        8,
        "each queue up to its own 6, together 8 jobs"
    );
}

/// B2.31: completion, pause, cancel, resume and queue stop/start release and reuse slots.
#[tokio::test]
async fn freed_capacity_is_reused_and_held_jobs_do_not_take_it() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_queue_limit("main", 6).await.unwrap();
    let second = core.create_queue("Second", Some(6)).await.unwrap();
    set_global(&core, 3).await;
    let main = add_many(&core, "reuse", 3, None).await;
    let other = add_many(&core, "other", 2, Some(&second.queue_id)).await;
    assert_eq!(active(&core).await, 3);
    complete(&core, dir.path(), &main[0]).await;
    assert_eq!(core.get(&main[0]).await.unwrap().phase, Phase::Completed);
    assert_eq!(
        active(&core).await,
        3,
        "a completion frees a slot that a waiting job takes"
    );
    core.pause(&main[1]).await.unwrap();
    assert_eq!(
        active(&core).await,
        3,
        "a pause frees a slot that the last waiting job takes"
    );
    core.cancel(&main[2]).await.unwrap();
    assert_eq!(active(&core).await, 2, "nothing is left waiting");
    core.resume(&main[1]).await.unwrap();
    assert_eq!(active(&core).await, 3, "a resumed job takes the free slot");
    core.set_queue_running(&second.queue_id, false)
        .await
        .unwrap();
    assert_eq!(
        active(&core).await,
        1,
        "stopping a queue holds its jobs and frees their slots"
    );
    for id in &other {
        assert!(
            core.get(id)
                .await
                .unwrap()
                .blockers
                .contains(&Blocker::QueueStopped)
        );
    }
    core.set_queue_running(&second.queue_id, true)
        .await
        .unwrap();
    assert_eq!(
        active(&core).await,
        3,
        "starting it again admits its jobs within the limit"
    );
    assert_eq!(
        core.get(&main[1]).await.unwrap().desired_state,
        DesiredState::Running
    );
}

/// B2.31: a restart keeps the global limit and the queue limits it was given.
#[tokio::test]
async fn a_restart_keeps_the_global_and_the_queue_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("limits.sqlite3");
    {
        let core = supported(dir.path())
            .with_store(Store::open(&path).unwrap())
            .unwrap();
        core.set_queue_limit("main", 6).await.unwrap();
        set_global(&core, 6).await;
    }
    let core = supported(dir.path())
        .with_store(Store::open(&path).unwrap())
        .unwrap();
    assert_eq!(core.concurrency_settings().await.max_active_downloads, 6);
    assert_eq!(core.queues().await[0].max_active, 6);
    add_many(&core, "restart", 8, None).await;
    assert_eq!(active(&core).await, 6);
}

/// B2.31: when the durable save fails, neither the saved nor the effective limit changes.
#[tokio::test]
async fn a_failed_durable_save_changes_neither_the_saved_nor_the_effective_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refused.sqlite3");
    {
        let core = supported(dir.path())
            .with_store(Store::open(&path).unwrap())
            .unwrap();
        core.set_queue_limit("main", 16).await.unwrap();
        set_global(&core, 5).await;
    }
    // The database refuses any further change of the setting (a full disk, a broken file…).
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_limit BEFORE UPDATE ON settings \
             WHEN NEW.key = 'max_active_downloads' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let core = supported(dir.path())
        .with_store(Store::open(&path).unwrap())
        .unwrap();
    let e = core
        .set_concurrency(ConcurrencySettings {
            max_active_downloads: 9,
        })
        .await
        .unwrap_err();
    assert_eq!(e.0.kind, ErrorKind::Internal);
    assert_eq!(core.concurrency_settings().await.max_active_downloads, 5);
    add_many(&core, "refused", 8, None).await;
    assert_eq!(
        active(&core).await,
        5,
        "admission still uses the saved limit"
    );
}
