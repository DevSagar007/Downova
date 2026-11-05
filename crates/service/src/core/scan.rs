//! The attachment check of a placed file (F07, E06, ADR 0026).
//!
//! The OS call (`IAttachmentExecute::Save` on Windows) runs on a thread of its own, never under the
//! coordinator lock: [`Slots::start`] queues it behind a small semaphore and the poll loop only
//! collects a finished result. Every check carries a ticket; cancelling a job revokes its ticket,
//! so a result that arrives later is dropped and can never complete or revive the job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use dm_domain::JobId;
use dm_platform::attachment::{self, Capability, SaveResult};

/// At most this many checks run at once (each on its own OS thread).
pub(crate) const MAX_CONCURRENT_SCANS: usize = 2;
/// A check without a result after this long completes as `unavailable/timed_out`; its late
/// result is discarded.
pub(crate) const SCAN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// The OS seam. Production uses [`OsScanner`]; tests inject deterministic fakes.
pub trait Scanner: Send + Sync + std::fmt::Debug + 'static {
    fn capability(&self) -> Capability;
    /// Blocking; runs on a dedicated thread.
    fn save(&self, path: &Path, origin: &str) -> SaveResult;
    fn named_streams_supported(&self, path: &Path) -> Option<bool>;
    fn zone_identifier_present(&self, path: &Path) -> Option<bool>;
    fn write_zone_identifier(&self, path: &Path, origin: &str) -> std::io::Result<()>;
    fn file_identity(&self, path: &Path) -> std::io::Result<String>;
}

/// The real adapter (`dm-platform::attachment`).
#[derive(Debug, Default)]
pub struct OsScanner;

impl Scanner for OsScanner {
    fn capability(&self) -> Capability {
        attachment::capability()
    }
    fn save(&self, path: &Path, origin: &str) -> SaveResult {
        attachment::save(path, origin)
    }
    fn named_streams_supported(&self, path: &Path) -> Option<bool> {
        attachment::named_streams_supported(path)
    }
    fn zone_identifier_present(&self, path: &Path) -> Option<bool> {
        attachment::zone_identifier_present(path)
    }
    fn write_zone_identifier(&self, path: &Path, origin: &str) -> std::io::Result<()> {
        attachment::write_zone_identifier(path, origin)
    }
    fn file_identity(&self, path: &Path) -> std::io::Result<String> {
        attachment::file_identity(path)
    }
}

/// A finished check, as collected by the poll loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Collected {
    Pending,
    Done(SaveResult),
    TimedOut,
    /// No check is known for this job (after a restart, or never started).
    Missing,
}

#[derive(Debug)]
struct Slot {
    ticket: u64,
    requested: tokio::time::Instant,
    result: Option<SaveResult>,
}

#[derive(Debug, Default)]
struct Inner {
    next: u64,
    slots: HashMap<JobId, Slot>,
}

/// In-flight checks, keyed by job.
#[derive(Debug, Clone)]
pub(crate) struct Slots {
    inner: Arc<StdMutex<Inner>>,
    permits: Arc<tokio::sync::Semaphore>,
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_SCANS)),
        }
    }
}

fn lock(m: &StdMutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Slots {
    /// Queues a check of `path` for `job` (replacing any earlier ticket of that job).
    pub(crate) fn start(
        &self,
        job: &JobId,
        path: PathBuf,
        origin: String,
        scanner: Arc<dyn Scanner>,
    ) {
        let ticket = {
            let mut inner = lock(&self.inner);
            inner.next = inner.next.wrapping_add(1);
            let ticket = inner.next;
            inner.slots.insert(
                job.clone(),
                Slot {
                    ticket,
                    requested: tokio::time::Instant::now(),
                    result: None,
                },
            );
            ticket
        };
        let (inner, permits, job) = (
            Arc::clone(&self.inner),
            Arc::clone(&self.permits),
            job.clone(),
        );
        tokio::spawn(async move {
            // Bounded: the permit is held until the OS call returns (a hung call keeps its permit).
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
            if !lock(&inner)
                .slots
                .get(&job)
                .is_some_and(|s| s.ticket == ticket)
            {
                return; // Revoked (cancelled) while queued: never run it.
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            let spawned = std::thread::Builder::new()
                .name("downova-attachment-check".into())
                .spawn(move || {
                    let _ = tx.send(scanner.save(&path, &origin));
                });
            let result = match spawned {
                Ok(_) => rx.await.unwrap_or(SaveResult::Failed(-1)),
                Err(_) => SaveResult::Failed(-1),
            };
            if let Some(slot) = lock(&inner).slots.get_mut(&job)
                && slot.ticket == ticket
            {
                slot.result = Some(result);
            }
        });
    }

    /// The check's state for `job`; a finished or timed-out check is taken out of the table.
    pub(crate) fn collect(&self, job: &JobId) -> Collected {
        let mut inner = lock(&self.inner);
        let Some(slot) = inner.slots.get(job) else {
            return Collected::Missing;
        };
        if let Some(result) = slot.result {
            inner.slots.remove(job);
            return Collected::Done(result);
        }
        if slot.requested.elapsed() >= SCAN_TIMEOUT {
            inner.slots.remove(job);
            return Collected::TimedOut;
        }
        Collected::Pending
    }

    /// Revokes `job`'s check: a later result is dropped.
    pub(crate) fn revoke(&self, job: &JobId) {
        lock(&self.inner).slots.remove(job);
    }

    /// Checks queued or running (diagnostics).
    pub(crate) fn in_flight(&self) -> usize {
        lock(&self.inner).slots.len()
    }
}
