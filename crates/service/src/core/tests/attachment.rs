//! F07/E06 (ADR 0026): the attachment check inside finalization, with deterministic fakes of the OS
//! seam. These prove the state machine; the real Windows API is exercised separately
//! (`dm-win-attachment`'s ignored test and the packaged run).
use super::*;
use crate::core::scan::Scanner;
use dm_domain::api::{OriginMarking, ScanReason, ScanRecord, ScanStatus};
use dm_platform::attachment::{Capability, SaveResult};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What the fake does when `save` is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Act {
    /// Returns this result and leaves the file alone.
    Return(SaveResult),
    /// Deletes the file (an antivirus quarantining it), then returns this.
    Delete(SaveResult),
    /// Rewrites the file's bytes, then returns this.
    Alter(SaveResult),
    /// Replaces the file by another one with the same bytes, then returns this.
    Replace(SaveResult),
}

#[derive(Debug)]
pub(super) struct FakeScanner {
    pub act: StdMutex<Act>,
    /// While set, `save` waits (a slow or hung trust service).
    pub hold: StdMutex<bool>,
    pub calls: AtomicUsize,
    pub origins: StdMutex<Vec<String>>,
    pub streams: Option<bool>,
    pub zone_written: StdMutex<Vec<String>>,
}

impl FakeScanner {
    pub fn new(act: Act) -> Arc<Self> {
        Arc::new(Self {
            act: StdMutex::new(act),
            hold: StdMutex::new(false),
            calls: AtomicUsize::new(0),
            origins: StdMutex::new(Vec::new()),
            streams: Some(true),
            zone_written: StdMutex::new(Vec::new()),
        })
    }
}

impl Scanner for FakeScanner {
    fn capability(&self) -> Capability {
        Capability::WindowsAttachmentServices
    }
    fn save(&self, path: &Path, origin: &str) -> SaveResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.origins.lock().unwrap().push(origin.to_owned());
        while *self.hold.lock().unwrap() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match *self.act.lock().unwrap() {
            Act::Return(r) => r,
            Act::Delete(r) => {
                let _ = std::fs::remove_file(path);
                r
            }
            Act::Alter(r) => {
                std::fs::write(path, b"cleaned by antivirus").unwrap();
                r
            }
            Act::Replace(r) => {
                let bytes = std::fs::read(path).unwrap();
                std::fs::remove_file(path).unwrap();
                std::fs::write(path, bytes).unwrap();
                r
            }
        }
    }
    fn named_streams_supported(&self, _path: &Path) -> Option<bool> {
        self.streams
    }
    fn zone_identifier_present(&self, path: &Path) -> Option<bool> {
        Some(
            self.zone_written
                .lock()
                .unwrap()
                .iter()
                .any(|p| p == &path.display().to_string()),
        )
    }
    fn write_zone_identifier(&self, path: &Path, _origin: &str) -> std::io::Result<()> {
        self.zone_written
            .lock()
            .unwrap()
            .push(path.display().to_string());
        Ok(())
    }
    fn file_identity(&self, path: &Path) -> std::io::Result<String> {
        dm_platform::attachment::file_identity(path)
    }
}

/// No attachment integration (macOS/Linux in this version).
#[derive(Debug)]
pub(super) struct NoScanner;
impl Scanner for NoScanner {
    fn capability(&self) -> Capability {
        Capability::NotSupported
    }
    fn save(&self, _: &Path, _: &str) -> SaveResult {
        SaveResult::NotSupported
    }
    fn named_streams_supported(&self, _: &Path) -> Option<bool> {
        None
    }
    fn zone_identifier_present(&self, _: &Path) -> Option<bool> {
        None
    }
    fn write_zone_identifier(&self, _: &Path, _: &str) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
    fn file_identity(&self, path: &Path) -> std::io::Result<String> {
        dm_platform::attachment::file_identity(path)
    }
}

const BODY: &[u8] = b"verified bytes of a finished download";

/// A job whose transfer completed with `BODY` staged; returns the snapshot after `polls` polls.
async fn finished(core: &Core<FakeBackend>, dir: &Path, key: &str) -> DownloadSnapshot {
    let s = core
        .add(add_params(
            key,
            &format!("https://user:SYNTH-PW@dl.example.org/f/{key}.bin?token=SYNTH-TOKEN#frag"),
        ))
        .await
        .unwrap();
    std::fs::write(dir.join(format!("{key}.bin.downova-part")), BODY).unwrap();
    core.backend().set_progress(
        &handle_of(&s),
        progress(
            TransferState::Complete,
            BODY.len() as u64,
            Some(BODY.len() as u64),
        ),
    );
    s
}

async fn settle(core: &Core<FakeBackend>, id: &JobId) -> DownloadSnapshot {
    for _ in 0..400 {
        core.poll_once().await;
        let s = core.get(id).await.unwrap();
        if s.phase.is_terminal() {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    core.get(id).await.unwrap()
}

fn scanned(dir: &Path, scanner: Arc<FakeScanner>) -> Core<FakeBackend> {
    core_with(FakeBackend::with_all(CapabilitySupport::Supported), dir).with_scanner(scanner)
}

#[tokio::test]
async fn a_checked_file_completes_with_honest_evidence_and_a_sanitized_origin() {
    let dir = tempfile::tempdir().unwrap();
    let scanner = FakeScanner::new(Act::Return(SaveResult::Saved));
    let core = scanned(dir.path(), Arc::clone(&scanner));
    let s = finished(&core, dir.path(), "att-ok-000001").await;
    core.poll_once().await;
    let mid = core.get(&s.job_id).await.unwrap();
    assert_eq!(mid.phase, Phase::Scanning, "not completed before the check");
    assert_eq!(mid.scan.unwrap().status, ScanStatus::Scanning);
    let done = settle(&core, &s.job_id).await;
    assert_eq!(done.phase, Phase::Completed);
    let scan = done.scan.unwrap();
    assert_eq!(scan.status, ScanStatus::AttachmentChecked);
    assert_eq!(scan.reason, None);
    assert!(
        done.verification.is_some(),
        "checksum evidence stays separate"
    );
    assert_eq!(
        scanner.origins.lock().unwrap().as_slice(),
        ["https://dl.example.org/"],
        "no userinfo, path, query or fragment reaches the OS"
    );
    assert_eq!(
        std::fs::read(dir.path().join("att-ok-000001.bin")).unwrap(),
        BODY
    );
    assert!(!dir.path().join("att-ok-000001.bin.downova-part").exists());
}

#[tokio::test]
async fn removed_altered_or_replaced_files_are_blocked_and_never_recreated() {
    for (act, reason) in [
        (Act::Delete(SaveResult::Saved), ScanReason::FileRemoved),
        (Act::Delete(SaveResult::Failed(-1)), ScanReason::FileRemoved),
        (Act::Alter(SaveResult::Saved), ScanReason::FileChanged),
        (Act::Replace(SaveResult::Saved), ScanReason::FileReplaced),
        (
            Act::Return(SaveResult::PolicyBlocked),
            ScanReason::PolicyBlocked,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let core = scanned(dir.path(), FakeScanner::new(act));
        let s = finished(&core, dir.path(), "att-block-0001").await;
        let done = settle(&core, &s.job_id).await;
        assert_eq!(done.phase, Phase::Failed, "{act:?}");
        assert_eq!(
            done.error.as_ref().unwrap().kind,
            ErrorKind::ScanBlocked,
            "{act:?}"
        );
        let scan = done.scan.unwrap();
        assert_eq!(
            (scan.status, scan.reason),
            (ScanStatus::Blocked, Some(reason)),
            "{act:?}"
        );
        assert!(
            done.verification.is_none(),
            "{act:?}: a blocked file is not verified"
        );
        // Our own second name of those bytes is gone; a removed file is not re-created.
        assert!(
            !dir.path().join("att-block-0001.bin.downova-part").exists(),
            "{act:?}"
        );
        if matches!(act, Act::Delete(_)) {
            assert!(!dir.path().join("att-block-0001.bin").exists(), "{act:?}");
        }
    }
}

#[tokio::test]
async fn an_unavailable_service_completes_with_a_warning_and_fallback_marking() {
    let dir = tempfile::tempdir().unwrap();
    let scanner = FakeScanner::new(Act::Return(SaveResult::Failed(0x8000_4005_u32 as i32)));
    let core = scanned(dir.path(), Arc::clone(&scanner));
    let s = finished(&core, dir.path(), "att-unav-0001").await;
    let done = settle(&core, &s.job_id).await;
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(
        done.scan,
        Some(ScanRecord {
            status: ScanStatus::Unavailable,
            reason: Some(ScanReason::ServiceError),
            origin_marking: Some(OriginMarking::WrittenByDownova),
        })
    );
    assert_eq!(scanner.zone_written.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_volume_without_streams_reports_it_and_gets_no_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = FakeScanner::new(Act::Return(SaveResult::Failed(-1)));
    Arc::get_mut(&mut fake).unwrap().streams = Some(false);
    let core = scanned(dir.path(), Arc::clone(&fake));
    let s = finished(&core, dir.path(), "att-fat-00001").await;
    let done = settle(&core, &s.job_id).await;
    let scan = done.scan.unwrap();
    assert_eq!(scan.status, ScanStatus::Unavailable);
    assert_eq!(scan.origin_marking, Some(OriginMarking::UnsupportedVolume));
    assert!(fake.zone_written.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_an_integration_the_check_is_recorded_as_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_with(
        FakeBackend::with_all(CapabilitySupport::Supported),
        dir.path(),
    )
    .with_scanner(Arc::new(NoScanner));
    let s = finished(&core, dir.path(), "att-skip-0001").await;
    let done = settle(&core, &s.job_id).await;
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(
        done.scan,
        Some(ScanRecord {
            status: ScanStatus::Skipped,
            reason: Some(ScanReason::PlatformNotSupported),
            origin_marking: Some(OriginMarking::NotSupportedOnPlatform),
        })
    );
}

#[tokio::test]
async fn cancelling_during_the_check_wins_over_its_late_result() {
    let dir = tempfile::tempdir().unwrap();
    let scanner = FakeScanner::new(Act::Return(SaveResult::Saved));
    *scanner.hold.lock().unwrap() = true;
    let core = scanned(dir.path(), Arc::clone(&scanner));
    let s = finished(&core, dir.path(), "att-cancel-001").await;
    core.poll_once().await;
    for _ in 0..200 {
        if scanner.calls.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(core.get(&s.job_id).await.unwrap().phase, Phase::Scanning);
    let c = core.cancel(&s.job_id).await.unwrap();
    assert_eq!(c.phase, Phase::Cancelled);
    assert_eq!(c.scan.unwrap().reason, Some(ScanReason::Cancelled));
    // The OS call returns afterwards: its result is dropped.
    *scanner.hold.lock().unwrap() = false;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    for _ in 0..5 {
        core.poll_once().await;
    }
    let after = core.get(&s.job_id).await.unwrap();
    assert_eq!(
        after.phase,
        Phase::Cancelled,
        "a late result never revives the job"
    );
    assert_eq!(after.scan.unwrap().status, ScanStatus::Skipped);
    assert!(after.verification.is_none());
}

#[tokio::test]
async fn a_hung_check_times_out_as_unavailable_and_others_keep_moving() {
    let dir = tempfile::tempdir().unwrap();
    let scanner = FakeScanner::new(Act::Return(SaveResult::Saved));
    *scanner.hold.lock().unwrap() = true;
    let core = scanned(dir.path(), Arc::clone(&scanner));
    let s = finished(&core, dir.path(), "att-hang-00001").await;
    core.poll_once().await;
    assert_eq!(core.get(&s.job_id).await.unwrap().phase, Phase::Scanning);
    // Other work is not blocked while the check hangs: a new download is admitted.
    let other = core
        .add(add_params("att-hang-other1", "http://h/other.bin"))
        .await
        .unwrap();
    assert!(
        core.backend()
            .calls()
            .iter()
            .any(|c| c.starts_with("start")
                && c.contains(other.attempt_id.as_ref().unwrap().as_str()))
    );
    tokio::time::pause();
    tokio::time::advance(crate::core::scan::SCAN_TIMEOUT).await;
    core.poll_once().await;
    let done = core.get(&s.job_id).await.unwrap();
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(done.scan.unwrap().reason, Some(ScanReason::TimedOut));
    *scanner.hold.lock().unwrap() = false;
}

#[tokio::test]
async fn a_restart_during_the_check_reverifies_and_checks_again_but_never_replaces_a_removed_file()
{
    for removed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("downova.sqlite3");
        let scanner = FakeScanner::new(Act::Return(SaveResult::Saved));
        *scanner.hold.lock().unwrap() = true;
        let id = {
            let core = scanned(dir.path(), Arc::clone(&scanner))
                .with_store(Store::open(&db).unwrap())
                .unwrap();
            let s = finished(&core, dir.path(), "att-restart-01").await;
            core.poll_once().await;
            assert_eq!(core.get(&s.job_id).await.unwrap().phase, Phase::Scanning);
            s.job_id
        };
        if removed {
            std::fs::remove_file(dir.path().join("att-restart-01.bin")).unwrap();
        }
        let fresh = FakeScanner::new(Act::Return(SaveResult::Saved));
        let core = core_with(
            FakeBackend::with_all(CapabilitySupport::Supported),
            dir.path(),
        )
        .with_scanner(Arc::clone(&fresh) as Arc<dyn Scanner>)
        .with_store(Store::open(&db).unwrap())
        .unwrap();
        core.recover().await;
        let done = settle(&core, &id).await;
        if removed {
            assert_eq!(done.phase, Phase::Failed);
            assert_eq!(done.scan.unwrap().reason, Some(ScanReason::FileRemoved));
            assert!(
                !dir.path().join("att-restart-01.bin").exists(),
                "never re-placed"
            );
            assert_eq!(
                fresh.calls.load(Ordering::SeqCst),
                0,
                "a removed file is not checked again"
            );
        } else {
            assert_eq!(done.phase, Phase::Completed);
            assert_eq!(done.scan.unwrap().status, ScanStatus::AttachmentChecked);
            assert_eq!(
                fresh.calls.load(Ordering::SeqCst),
                1,
                "checked again after the restart"
            );
        }
        *scanner.hold.lock().unwrap() = false;
    }
}

#[tokio::test]
async fn a_blocked_download_notifies_exactly_once_and_only_after_the_outcome() {
    use crate::notify::recording::RecordingNotifier;
    let dir = tempfile::tempdir().unwrap();
    let notifier = Arc::new(RecordingNotifier::default());
    let scanner = FakeScanner::new(Act::Delete(SaveResult::Saved));
    *scanner.hold.lock().unwrap() = true;
    let core = scanned(dir.path(), Arc::clone(&scanner)).with_notifier(notifier.clone());
    core.set_notifications(dm_domain::api::NotificationSettings {
        enabled: true,
        locale: dm_domain::api::NotificationLocale::En,
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // Turning notifications on shows one confirmation notice (ADR 0022).
    let base = notifier.shown.lock().unwrap().len();
    let s = finished(&core, dir.path(), "att-notify-001").await;
    for _ in 0..3 {
        core.poll_once().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        notifier.shown.lock().unwrap().len() == base,
        "nothing while the check runs"
    );
    *scanner.hold.lock().unwrap() = false;
    settle(&core, &s.job_id).await;
    for _ in 0..5 {
        core.poll_once().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        notifier.shown.lock().unwrap().len(),
        base + 1,
        "one failure notice, never twice"
    );
}
