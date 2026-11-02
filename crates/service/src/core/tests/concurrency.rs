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

#[tokio::test]
async fn the_optional_aria2_backend_keeps_its_conservative_cap() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = FakeBackend::with_all(CapabilitySupport::Supported);
    fake.identity = Some("aria2");
    let core = core_with(fake, dir.path());
    core.set_queue_limit("main", 16).await.unwrap();
    core.set_concurrency(ConcurrencySettings {
        max_active_downloads: 16,
    })
    .await
    .unwrap();
    for i in 0..8 {
        core.add(add_params(
            &format!("aria-limit-{i:04}"),
            &format!("http://h/{i}.bin"),
        ))
        .await
        .unwrap();
    }
    assert_eq!(active(&core).await, 3);
    assert_eq!(
        core.concurrency_settings().await.max_active_downloads,
        16,
        "saved setting remains available to the normal backend"
    );
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
