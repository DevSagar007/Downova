//! B2.8 (duplicates, destination rules, names) and B2.11 (queues, priority, order) behaviour of
//! the coordinator, against the fake backend.

use super::*;
use dm_domain::api::{DestinationRule, MoveTarget, ServiceSettings};
use dm_domain::policy::{CollisionPolicy, DuplicatePolicy, FileCategory, Priority};

fn refuse(key: &str, url: &str) -> AddDownloadParams {
    AddDownloadParams {
        on_duplicate: Some(DuplicatePolicy::Refuse),
        ..add_params(key, url)
    }
}

fn supported(dir: &Path) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir)
}

fn destination_of(core: &Core<FakeBackend>, job: &DownloadSnapshot) -> std::path::PathBuf {
    let started = core.backend().started.lock().unwrap();
    started
        .iter()
        .find(|r| Some(&r.attempt_id) == job.attempt_id.as_ref())
        .map(|r| r.destination_dir.clone())
        .unwrap()
}

// ------------------------------------------------------------------ duplicates

#[tokio::test]
async fn check_lists_the_same_link_and_refuse_blocks_a_second_job() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let url = "https://Host.example/Files/a.bin?sig=SECRET-ONE";
    let first = core.add(add_params("dup-key-0001", url)).await.unwrap();

    // Same identity (scheme and host case only differ).
    let found = core
        .check("HTTPS://host.EXAMPLE/Files/a.bin?sig=SECRET-ONE")
        .await
        .unwrap();
    assert_eq!(found.duplicates.len(), 1);
    assert_eq!(found.duplicates[0].job_id, first.job_id);
    assert!(!serde_json::to_string(&found).unwrap().contains("SECRET"));
    // Path case and a different signature are different links.
    for other in [
        "https://host.example/files/a.bin?sig=SECRET-ONE",
        "https://host.example/Files/a.bin?sig=SECRET-TWO",
    ] {
        assert!(
            core.check(other).await.unwrap().duplicates.is_empty(),
            "{other}"
        );
    }

    let refused = core.add(refuse("dup-key-0002", url)).await.unwrap_err();
    assert_eq!(refused.0.kind, ErrorKind::DuplicateDownload);
    assert!(!refused.0.detail.unwrap_or_default().contains("SECRET"));
    assert_eq!(core.list().await.len(), 1, "nothing was created");
    // Omitted policy (CLI, browser host) and explicit allow add another job.
    let second = core.add(add_params("dup-key-0003", url)).await.unwrap();
    assert_ne!(second.job_id, first.job_id);
    assert_eq!(core.check(url).await.unwrap().duplicates.len(), 2);
}

#[tokio::test]
async fn a_replay_with_the_same_key_returns_the_original_job_even_when_refusing() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let url = "http://h/same.bin";
    let first = core.add(refuse("replay-key-01", url)).await.unwrap();
    // Idempotency comes first: same key + same request is the same job, not a duplicate.
    let again = core.add(refuse("replay-key-01", url)).await.unwrap();
    assert_eq!(again.job_id, first.job_id);
    assert_eq!(core.list().await.len(), 1);
}

#[tokio::test]
async fn concurrent_refusing_adds_of_one_link_create_exactly_one_job() {
    let dir = tempfile::tempdir().unwrap();
    let core = std::sync::Arc::new(supported(dir.path()));
    let url = "http://h/race.bin";
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let core = std::sync::Arc::clone(&core);
            tokio::spawn(async move { core.add(refuse(&format!("race-key-{i:04}"), url)).await })
        })
        .collect();
    let mut ok = 0;
    let mut refused = 0;
    for t in tasks {
        match t.await.unwrap() {
            Ok(_) => ok += 1,
            Err(e) => {
                assert_eq!(e.0.kind, ErrorKind::DuplicateDownload);
                refused += 1;
            }
        }
    }
    assert_eq!((ok, refused), (1, 7));
    assert_eq!(core.list().await.len(), 1);
}

#[tokio::test]
async fn removed_jobs_are_not_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let url = "http://h/old.bin";
    let job = core.add(add_params("removed-key-1", url)).await.unwrap();
    core.cancel(&job.job_id).await.unwrap();
    core.remove(&job.job_id).await.unwrap();
    assert!(core.check(url).await.unwrap().duplicates.is_empty());
    core.add(refuse("removed-key-2", url)).await.unwrap();
}

#[test]
fn requests_without_new_fields_keep_their_old_fingerprint() {
    use sha2::{Digest, Sha256};
    let params = add_params("legacy-key-1", "https://h/f.zip?x=1");
    // The B1.1 canonical form, computed independently.
    let legacy = serde_json::json!(["https://h/f.zip?x=1", "downloads", null, null]).to_string();
    let expected: String = Sha256::digest(legacy.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(fingerprint_sha256(&params), expected);
    // A new field changes it (the same key with a different request is a conflict).
    let with_policy = AddDownloadParams {
        on_duplicate: Some(DuplicatePolicy::Refuse),
        ..params
    };
    assert_ne!(fingerprint_sha256(&with_policy), expected);
}

// ------------------------------------------------------------------ destination rules

#[tokio::test]
async fn a_rule_chooses_the_folder_at_acceptance_and_later_edits_never_move_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let videos = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let core = stored_core(&db, dir.path());
    let rules = |d: &Path| ServiceSettings {
        destination_rules: vec![DestinationRule {
            category: FileCategory::Video,
            directory: d.display().to_string(),
        }],
        on_collision: CollisionPolicy::Rename,
    };
    core.set_settings(rules(videos.path())).await.unwrap();

    let clip = core
        .add(add_params("rule-key-001", "http://h/clip.MKV"))
        .await
        .unwrap();
    assert_eq!(clip.category, Some(FileCategory::Video));
    assert_eq!(destination_of(&core, &clip), videos.path());
    let doc = core
        .add(add_params("rule-key-002", "http://h/a.pdf"))
        .await
        .unwrap();
    assert_eq!(
        destination_of(&core, &doc),
        dir.path(),
        "no rule: profile folder"
    );
    // An explicit category wins over the link's extension.
    let explicit = core
        .add(AddDownloadParams {
            category: Some(FileCategory::Video),
            ..add_params("rule-key-003", "http://h/archive.zip")
        })
        .await
        .unwrap();
    assert_eq!(destination_of(&core, &explicit), videos.path());

    core.set_settings(rules(other.path())).await.unwrap();
    // The stored job keeps its folder (its partial file and checkpoint live there).
    {
        let state = core.state.lock().await;
        let job = state.jobs.iter().find(|j| j.id == clip.job_id).unwrap();
        assert_eq!(job.destination, videos.path());
    }
    drop(core);
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(
        core.settings().await,
        rules(other.path()),
        "settings are durable"
    );
    let state = core.state.lock().await;
    let job = state.jobs.iter().find(|j| j.id == clip.job_id).unwrap();
    assert_eq!(job.destination, videos.path());
    assert_eq!(job.category, Some(FileCategory::Video));
}

#[tokio::test]
async fn destination_folders_are_validated_without_echoing_them() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let set = |d: &str| ServiceSettings {
        destination_rules: vec![DestinationRule {
            category: FileCategory::Music,
            directory: d.to_owned(),
        }],
        on_collision: CollisionPolicy::Rename,
    };
    let relative = core.set_settings(set("music/here")).await.unwrap_err();
    assert_eq!(relative.0.kind, ErrorKind::InvalidRequest);
    let dotted = dir.path().join("..").join("x").display().to_string();
    let parent = core.set_settings(set(&dotted)).await.unwrap_err();
    assert_eq!(parent.0.kind, ErrorKind::InvalidRequest);
    let missing = dir.path().join("SECRET-missing").display().to_string();
    let gone = core.set_settings(set(&missing)).await.unwrap_err();
    assert_eq!(gone.0.kind, ErrorKind::VolumeMissing);
    assert!(!gone.0.detail.unwrap_or_default().contains("SECRET"));
    let twice = ServiceSettings {
        destination_rules: vec![
            DestinationRule {
                category: FileCategory::Music,
                directory: dir.path().display().to_string(),
            },
            DestinationRule {
                category: FileCategory::Music,
                directory: dir.path().display().to_string(),
            },
        ],
        on_collision: CollisionPolicy::Ask,
    };
    assert!(core.set_settings(twice).await.is_err());
    assert_eq!(
        core.settings().await,
        ServiceSettings::default(),
        "nothing changed"
    );
}

// ------------------------------------------------------------------ names

fn complete_with_name(core: &Core<FakeBackend>, job: &DownloadSnapshot, cd: &str) {
    core.backend().set_progress(
        &handle_of(job),
        BackendProgress {
            naming: Some(ResponseNaming {
                content_disposition: Some(cd.to_owned()),
                url_file_name: None,
            }),
            ..progress(TransferState::Active, 5, Some(5))
        },
    );
}

#[tokio::test]
async fn ask_on_collision_holds_the_verified_file_until_confirmed_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_settings(ServiceSettings {
        destination_rules: vec![],
        on_collision: CollisionPolicy::Ask,
    })
    .await
    .unwrap();
    std::fs::write(dir.path().join("report.txt"), b"theirs").unwrap();
    let job = core
        .add(add_params("ask-key-0001", "http://h/dl"))
        .await
        .unwrap();
    complete_with_name(&core, &job, "attachment; filename=\"report.txt\"");
    core.poll_once().await; // the name is chosen: report (1).txt, unconfirmed
    let staging = core.get(&job.job_id).await.unwrap();
    std::fs::write(staged(dir.path(), &job), b"ours!").unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, 5, Some(5)),
    );
    core.poll_once().await;
    let held = core.get(&job.job_id).await.unwrap();
    assert_eq!(held.phase, Phase::Verifying);
    assert_eq!(held.blockers, vec![Blocker::NameConflict]);
    assert_eq!(held.display_name, "report (1).txt");
    assert!(
        !dir.path().join("report (1).txt").exists(),
        "nothing placed yet"
    );
    core.poll_once().await; // still held
    assert_eq!(core.get(&job.job_id).await.unwrap().phase, Phase::Verifying);

    let confirmed = core.confirm_name(&job.job_id).await.unwrap();
    assert!(confirmed.blockers.is_empty());
    core.poll_once().await;
    let done = core.get(&job.job_id).await.unwrap();
    assert_eq!(done.phase, Phase::Completed, "{staging:?}");
    assert_eq!(
        std::fs::read(dir.path().join("report.txt")).unwrap(),
        b"theirs"
    );
    assert_eq!(
        std::fs::read(dir.path().join("report (1).txt")).unwrap(),
        b"ours!"
    );
}

#[tokio::test]
async fn cancelling_a_held_job_keeps_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    core.set_settings(ServiceSettings {
        destination_rules: vec![],
        on_collision: CollisionPolicy::Ask,
    })
    .await
    .unwrap();
    std::fs::write(dir.path().join("r.txt"), b"theirs").unwrap();
    let job = core
        .add(add_params("ask-key-0002", "http://h/r.txt"))
        .await
        .unwrap();
    std::fs::write(staged(dir.path(), &job), b"ours!").unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, 5, Some(5)),
    );
    core.poll_once().await;
    assert_eq!(
        core.get(&job.job_id).await.unwrap().blockers,
        vec![Blocker::NameConflict]
    );
    let cancelled = core.cancel(&job.job_id).await.unwrap();
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert_eq!(std::fs::read(dir.path().join("r.txt")).unwrap(), b"theirs");
}

#[tokio::test]
async fn a_name_taken_after_selection_is_replaced_by_the_next_free_one() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let job = core
        .add(add_params("race-name-01", "http://h/dl"))
        .await
        .unwrap();
    complete_with_name(&core, &job, "attachment; filename=\"data.bin\"");
    core.poll_once().await;
    assert_eq!(
        core.get(&job.job_id).await.unwrap().display_name,
        "data.bin"
    );
    // Someone else's file appears at the chosen name before placement.
    std::fs::write(dir.path().join("data.bin"), b"intruder").unwrap();
    std::fs::write(staged(dir.path(), &job), b"ours!").unwrap();
    core.backend().set_progress(
        &handle_of(&job),
        progress(TransferState::Complete, 5, Some(5)),
    );
    core.poll_once().await;
    let done = core.get(&job.job_id).await.unwrap();
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(done.display_name, "data (1).bin");
    assert_eq!(
        std::fs::read(dir.path().join("data.bin")).unwrap(),
        b"intruder"
    );
    assert_eq!(
        std::fs::read(dir.path().join("data (1).bin")).unwrap(),
        b"ours!"
    );
}

// ------------------------------------------------------------------ queues and priority

#[tokio::test]
async fn queues_are_created_renamed_and_deleted_without_losing_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let core = stored_core(&db, dir.path());
    let night = core.create_queue("Night", Some(2)).await.unwrap();
    assert!(
        core.create_queue(" night ", None).await.is_err(),
        "names are unique"
    );
    assert!(core.create_queue("", None).await.is_err());
    assert!(core.create_queue("Big", Some(17)).await.is_err());
    let renamed = core
        .rename_queue(&night.queue_id, "Overnight")
        .await
        .unwrap();
    assert_eq!(renamed.name, "Overnight");
    let job = core
        .add(AddDownloadParams {
            queue_id: Some(night.queue_id.clone()),
            ..add_params("queue-key-01", "http://h/q.bin")
        })
        .await
        .unwrap();
    assert_eq!(job.queue_id.as_deref(), Some(night.queue_id.as_str()));
    assert!(
        core.add(AddDownloadParams {
            queue_id: Some("q_missing".into()),
            ..add_params("queue-key-02", "http://h/x.bin")
        })
        .await
        .is_err()
    );

    assert!(core.delete_queue(DEFAULT_QUEUE, None).await.is_err());
    assert!(
        core.delete_queue(&night.queue_id, None).await.is_err(),
        "not empty"
    );
    assert!(
        core.delete_queue(&night.queue_id, Some(&night.queue_id))
            .await
            .is_err()
    );
    core.delete_queue(&night.queue_id, Some(DEFAULT_QUEUE))
        .await
        .unwrap();
    let moved = core.get(&job.job_id).await.unwrap();
    assert_eq!(moved.queue_id.as_deref(), Some(DEFAULT_QUEUE));
    drop(core);
    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    assert_eq!(core.queues().await.len(), 1);
    assert_eq!(core.list().await.len(), 1, "the job survived its queue");
}

#[tokio::test]
async fn priority_wins_and_ties_keep_manual_order() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path()).with_max_active(1);
    let a = core
        .add(add_params("prio-key-0a", "http://h/a"))
        .await
        .unwrap();
    let b = core
        .add(add_params("prio-key-0b", "http://h/b"))
        .await
        .unwrap();
    let c = core
        .add(add_params("prio-key-0c", "http://h/c"))
        .await
        .unwrap();
    let d = core
        .add(add_params("prio-key-0d", "http://h/d"))
        .await
        .unwrap();
    assert!(a.attempt_id.is_some() && b.attempt_id.is_none());
    core.set_priority(&c.job_id, Priority::High).await.unwrap();
    core.move_job(&d.job_id, MoveTarget::Top).await.unwrap();
    // a finishes: c (high) starts, although b and d are ahead of it in the queue.
    core.pause(&a.job_id).await.unwrap();
    assert!(core.get(&c.job_id).await.unwrap().attempt_id.is_some());
    assert!(core.get(&b.job_id).await.unwrap().attempt_id.is_none());
    // Then, at equal priority, manual order: d (moved to the top) before b.
    core.pause(&c.job_id).await.unwrap();
    assert!(core.get(&d.job_id).await.unwrap().attempt_id.is_some());
    assert!(core.get(&b.job_id).await.unwrap().attempt_id.is_none());
}

#[tokio::test]
async fn manual_order_moves_up_down_top_and_bottom() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let mut ids = Vec::new();
    for i in 0..4 {
        let j = core
            .add(add_params(
                &format!("order-key-{i:03}"),
                &format!("http://h/{i}"),
            ))
            .await
            .unwrap();
        ids.push(j.job_id);
    }
    let order = |list: Vec<DownloadSnapshot>| {
        let mut l: Vec<_> = list
            .into_iter()
            .map(|s| (s.queue_position.unwrap().0, s.job_id))
            .collect();
        l.sort();
        l.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };
    core.move_job(&ids[3], MoveTarget::Up).await.unwrap();
    assert_eq!(
        order(core.list().await),
        vec![
            ids[0].clone(),
            ids[1].clone(),
            ids[3].clone(),
            ids[2].clone()
        ]
    );
    core.move_job(&ids[0], MoveTarget::Bottom).await.unwrap();
    core.move_job(&ids[2], MoveTarget::Top).await.unwrap();
    core.move_job(&ids[1], MoveTarget::Down).await.unwrap();
    assert_eq!(
        order(core.list().await),
        vec![
            ids[2].clone(),
            ids[3].clone(),
            ids[1].clone(),
            ids[0].clone()
        ]
    );
    // Moving past either end is a no-op.
    core.move_job(&ids[2], MoveTarget::Up).await.unwrap();
    core.move_job(&ids[0], MoveTarget::Down).await.unwrap();
    assert_eq!(
        order(core.list().await),
        vec![
            ids[2].clone(),
            ids[3].clone(),
            ids[1].clone(),
            ids[0].clone()
        ]
    );
}

#[tokio::test]
async fn a_queue_limit_applies_within_the_global_limit() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let slow = core.create_queue("Slow", Some(1)).await.unwrap();
    let in_slow = |k: &str, u: &str| AddDownloadParams {
        queue_id: Some(slow.queue_id.clone()),
        ..add_params(k, u)
    };
    let s1 = core
        .add(in_slow("limit-key-01", "http://h/1"))
        .await
        .unwrap();
    let s2 = core
        .add(in_slow("limit-key-02", "http://h/2"))
        .await
        .unwrap();
    let m1 = core
        .add(add_params("limit-key-03", "http://h/3"))
        .await
        .unwrap();
    assert!(s1.attempt_id.is_some());
    assert!(s2.attempt_id.is_none(), "the queue's single slot is taken");
    assert!(
        m1.attempt_id.is_some(),
        "another queue is not blocked by it"
    );
    // Raising the limit starts the waiting job; lowering it never stops a running one.
    core.set_queue_limit(&slow.queue_id, 2).await.unwrap();
    assert!(core.get(&s2.job_id).await.unwrap().attempt_id.is_some());
    core.set_queue_limit(&slow.queue_id, 1).await.unwrap();
    let still = core.get(&s1.job_id).await.unwrap();
    assert!(still.attempt_id.is_some());
    assert!(
        !core
            .backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("pause"))
    );
}

#[tokio::test]
async fn stop_queue_holds_without_overriding_user_intent_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downova.sqlite3");
    let core = stored_core(&db, dir.path());
    let running = core
        .add(add_params("stop-key-001", "http://h/run"))
        .await
        .unwrap();
    let paused = core
        .add(add_params("stop-key-002", "http://h/pause"))
        .await
        .unwrap();
    core.pause(&paused.job_id).await.unwrap();

    core.set_queue_running(DEFAULT_QUEUE, false).await.unwrap();
    let held = core.get(&running.job_id).await.unwrap();
    assert_eq!(
        held.desired_state,
        DesiredState::Running,
        "intent untouched"
    );
    assert!(held.blockers.contains(&Blocker::QueueStopped));
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("pause"))
    );
    // A user resume while the queue is stopped records intent but does not start anything.
    core.resume(&paused.job_id).await.unwrap();
    core.pause(&paused.job_id).await.unwrap();
    let late = core
        .add(add_params("stop-key-003", "http://h/late"))
        .await
        .unwrap();
    assert!(late.attempt_id.is_none() && late.blockers.contains(&Blocker::QueueStopped));
    drop(core);

    let core = reopen(
        &db,
        dir.path(),
        FakeBackend::with_all(CapabilitySupport::Supported),
    );
    core.recover().await;
    assert!(!core.queues().await[0].running, "stop is durable");
    assert!(
        core.get(&late.job_id)
            .await
            .unwrap()
            .blockers
            .contains(&Blocker::QueueStopped)
    );
    core.set_queue_running(DEFAULT_QUEUE, true).await.unwrap();
    let started = core.get(&late.job_id).await.unwrap();
    assert!(!started.blockers.contains(&Blocker::QueueStopped));
    assert!(started.attempt_id.is_some(), "a queued job starts");
    let still_paused = core.get(&paused.job_id).await.unwrap();
    assert_eq!(
        still_paused.desired_state,
        DesiredState::Paused,
        "start never resumes a user pause"
    );
    assert!(
        still_paused.attempt_id.is_none()
            || !core
                .backend()
                .calls()
                .iter()
                .any(|c| c == &format!("resume {}", still_paused.attempt_id.clone().unwrap()))
    );
}

#[tokio::test]
async fn moving_an_active_job_into_a_stopped_queue_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let parked = core.create_queue("Parked", None).await.unwrap();
    core.set_queue_running(&parked.queue_id, false)
        .await
        .unwrap();
    let job = core
        .add(add_params("move-key-0001", "http://h/m"))
        .await
        .unwrap();
    assert!(job.attempt_id.is_some());
    let moved = core
        .set_job_queue(&job.job_id, &parked.queue_id)
        .await
        .unwrap();
    assert!(moved.blockers.contains(&Blocker::QueueStopped));
    assert_eq!(moved.desired_state, DesiredState::Running);
    core.set_queue_running(&parked.queue_id, true)
        .await
        .unwrap();
    let back = core.get(&job.job_id).await.unwrap();
    assert!(!back.blockers.contains(&Blocker::QueueStopped));
}

// ------------------------------------------------------------------ B2.10 wake re-evaluation

#[test]
fn only_a_wall_clock_jump_well_past_the_interval_counts_as_a_wake() {
    use crate::server::{POLL_INTERVAL, WAKE_GAP, woke};
    assert!(!woke(POLL_INTERVAL));
    assert!(!woke(POLL_INTERVAL + WAKE_GAP));
    assert!(woke(
        POLL_INTERVAL + WAKE_GAP + std::time::Duration::from_millis(1)
    ));
    assert!(woke(std::time::Duration::from_secs(3_600)));
}

#[tokio::test(start_paused = true)]
async fn a_wake_makes_network_retries_due_but_keeps_server_cooldowns_and_pauses() {
    let dir = tempfile::tempdir().unwrap();
    let core = supported(dir.path());
    let offline = core
        .add(add_params("wake-key-001", "http://h/o"))
        .await
        .unwrap();
    let busy = core
        .add(add_params("wake-key-002", "http://h/b"))
        .await
        .unwrap();
    core.backend().set_progress(
        &handle_of(&offline),
        failed(ErrorKind::NetworkOffline, None),
    );
    core.backend()
        .set_progress(&handle_of(&busy), failed(ErrorKind::ServerBusy, Some(600)));
    core.poll_once().await;
    let starts_before = starts(&core);
    assert_eq!(
        core.on_wake().await,
        1,
        "only the network retry is re-evaluated"
    );
    core.poll_once().await; // the offline job is due now and queued again
    core.poll_once().await;
    assert!(
        starts(&core) > starts_before,
        "the network retry started at once"
    );
    let busy_now = core.get(&busy.job_id).await.unwrap();
    assert!(
        busy_now.blockers.contains(&Blocker::RateLimitCooldown),
        "Retry-After kept"
    );
}
