//! Rust HTTP/HTTPS backend prototype (ADR 0004), behind [`dm_backend_api::DownloadBackend`].
//!
//! This first increment (B0.4.2) transfers whole files over one connection:
//! - **Transport:** Tokio + reqwest (HTTP/1.1 only, no proxy, no cookie store, no automatic
//!   decompression, no hidden retries) with a rustls `ClientConfig` built here: the `ring`
//!   provider and the platform verifier (OS trust store), plus explicit extra roots for fixtures.
//!   Certificate verification is never disabled.
//! - **Redirects** are followed manually (bounded). Every hop's scheme is gated by the capability
//!   report, so `http://` → `https://` cannot bypass the `Https` gate, and credentials and
//!   validators are sent only to the request's original origin.
//! - **Responses:** `200` only, `Accept-Encoding: identity` with any encoded body refused, the byte
//!   count checked against `Content-Length`. Errors keep their status class and `Retry-After`; the
//!   service owns retries.
//! - **Files:** bytes stream into a backend-owned `<name>.downova-part` (created exclusively) in
//!   the destination directory. A finished transfer is synced and left there as its
//!   [`StagedOutput::Staged`] file; the service verifies it and places it at `<name>` through its
//!   completion journal (B1.3, ADR 0008). Cancel, pause and remove stop network and disk activity
//!   before they return; discard deletes only the attempt's own partial file.
//! - **Validated resume (B0.4.2, second increment):** the first `200` fixes the resource identity
//!   (strong ETag, length, final URL). A pause flushes and records a checkpoint (confirmed offset
//!   and the SHA-256 state of the bytes on disk). Resume re-verifies the partial file, then asks the
//!   final URL for `Range: bytes=<offset>-` with `If-Range: <ETag>` and appends only a `206` whose
//!   status, `Content-Range`, ETag, encoding and length all match. A `200` is never appended: it
//!   restarts the file from zero. Weak or missing ETags make resume explicitly unavailable.
//! - **Validated segmentation (B0.4.3):** with a strong ETag, a known length and
//!   `max_connections > 1`, the file is split into at most four contiguous segments of at least
//!   1 MiB; every segment is a validated `206` (see [`segmented`]). Small or unsuitable resources
//!   stay on the single-connection path.
//! - **Durable checkpoints (B1.2, ADR 0007):** running writers flush, sync and publish their
//!   confirmed state at least every second (or 4 MiB); [`DownloadBackend::checkpoint`] exports it
//!   as a versioned, size-bounded payload and [`DownloadBackend::restore`] re-imports it after a
//!   service restart, re-verifying every confirmed region on disk (see [`durable`]).

mod capabilities;
mod classify;
mod durable;
mod segmented;
mod transfer;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, DurableCheckpoint, StagedOutput,
    TransferRequest, TransferState,
};
use dm_domain::{ByteCount, CapabilityReport, ErrorKind};
use rustls::pki_types::CertificateDer;
use tokio::sync::watch;

pub use capabilities::report as capability_report;

/// Suffix of the backend-owned partial file next to the destination.
pub const PART_SUFFIX: &str = ".downova-part";

/// Backend construction options.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Extra trust anchors added to the platform's roots (test fixtures). Empty in production.
    pub extra_roots: Vec<CertificateDer<'static>>,
    /// Limit for establishing a connection (TCP + TLS).
    pub connect_timeout: Duration,
    /// Limit for each read while waiting for headers or body bytes.
    pub read_timeout: Duration,
    /// Maximum redirects followed for one attempt.
    pub max_redirects: u8,
    /// Report used to gate the scheme of every redirect hop. Defaults to this backend's own
    /// [`capability_report`].
    pub scheme_gate: Vec<CapabilityReport>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            extra_roots: Vec::new(),
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(30),
            max_redirects: 10,
            scheme_gate: capabilities::report(),
        }
    }
}

/// Live state of one attempt, shared with its transfer task.
#[derive(Debug)]
struct Shared {
    state: TransferState,
    completed: u64,
    total: Option<u64>,
}

struct Attempt {
    request: TransferRequest,
    part: PathBuf,
    shared: Arc<Mutex<Shared>>,
    control: Mutex<Option<Running>>,
    /// Latest confirmed on-disk state (written when the task stops).
    checkpoint: Mutex<Option<transfer::Checkpoint>>,
    /// Set by `resume` for the next task: continue from this checkpoint.
    resume_from: Mutex<Option<transfer::Checkpoint>>,
    /// Segmented transfers: confirmed per-segment state, and the resume request.
    seg_checkpoint: Mutex<Option<segmented::SegCheckpoint>>,
    seg_resume_from: Mutex<Option<segmented::SegCheckpoint>>,
}

impl Attempt {
    fn new(request: TransferRequest, part: PathBuf, shared: Shared) -> Self {
        Self {
            request,
            part,
            shared: Arc::new(Mutex::new(shared)),
            control: Mutex::new(None),
            checkpoint: Mutex::new(None),
            resume_from: Mutex::new(None),
            seg_checkpoint: Mutex::new(None),
            seg_resume_from: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for Attempt {
    // Never the raw URL or header values: the URL may carry signed-query tokens.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attempt")
            .field("attempt_id", &self.request.attempt_id)
            .field("source", &dm_domain::redact::redact_url(&self.request.url))
            .field("file_name", &self.request.file_name)
            .field("shared", &self.shared)
            .field("checkpoint", &self.checkpoint)
            .field("segments", &self.seg_checkpoint)
            .finish_non_exhaustive()
    }
}

/// A running transfer task and the switch that stops it.
#[derive(Debug)]
struct Running {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

/// The backend.
#[derive(Debug)]
pub struct HttpBackend {
    client: reqwest::Client,
    config: HttpConfig,
    attempts: Mutex<HashMap<String, Arc<Attempt>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A poisoned lock only means another thread panicked mid-update of plain data; the data is
    // still the latest written value, so continue with it rather than cascade the panic.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl HttpBackend {
    /// Builds the HTTP client (TLS configuration included).
    pub fn new(config: HttpConfig) -> Result<Self, BackendError> {
        let client = transfer::build_client(&config)?;
        Ok(Self {
            client,
            config,
            attempts: Mutex::new(HashMap::new()),
        })
    }

    /// Crate version, reported as the backend version.
    pub fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Stops every running transfer (network and disk) before returning.
    pub async fn shutdown(&self) -> Result<(), BackendError> {
        let all: Vec<Arc<Attempt>> = lock(&self.attempts).values().cloned().collect();
        for attempt in all {
            stop(&attempt, TransferState::Removed).await;
        }
        Ok(())
    }

    fn attempt(&self, handle: &BackendHandle) -> Result<Arc<Attempt>, BackendError> {
        lock(&self.attempts)
            .get(&handle.0)
            .cloned()
            .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "unknown attempt"))
    }

    fn spawn(&self, attempt: &Arc<Attempt>) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let task = tokio::spawn(transfer::run(
            self.client.clone(),
            self.config.clone(),
            Arc::clone(attempt),
            stop_rx,
        ));
        *lock(&attempt.control) = Some(Running {
            stop: stop_tx,
            task,
        });
    }
}

/// Signals the task to stop and waits until it has finished (connection dropped, file closed).
/// If the task is still running afterwards, `state` becomes the attempt's state.
async fn stop(attempt: &Attempt, state: TransferState) {
    let running = lock(&attempt.control).take();
    if let Some(Running { stop, mut task }) = running {
        let _ = stop.send(true);
        if tokio::time::timeout(Duration::from_secs(10), &mut task)
            .await
            .is_err()
        {
            // Bounded: a task stuck in a write is aborted; dropping its future closes the file
            // and the connection.
            task.abort();
            let _ = task.await;
        }
    }
    let mut shared = lock(&attempt.shared);
    if matches!(
        shared.state,
        TransferState::Active | TransferState::Waiting | TransferState::Paused
    ) {
        shared.state = state;
    }
}

/// Checks shared by `start` and `restore`; returns the destination and partial-file paths.
fn prepare(
    request: &TransferRequest,
    config: &HttpConfig,
    attempts: &Mutex<HashMap<String, Arc<Attempt>>>,
) -> Result<(PathBuf, PathBuf), BackendError> {
    validate_file_name(&request.file_name)?;
    if request.headers.iter().any(|(name, _)| {
        name.eq_ignore_ascii_case("range") || name.eq_ignore_ascii_case("if-range")
    }) {
        // Ranges and their validators are owned by the backend (resume, segments).
        return Err(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "caller-supplied Range or If-Range headers are not accepted",
        ));
    }
    transfer::gate_url(&request.url, &config.scheme_gate)?;
    if lock(attempts).contains_key(request.attempt_id.as_str()) {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "attempt already started",
        ));
    }
    let destination = request.destination_dir.join(&request.file_name);
    let part = request
        .destination_dir
        .join(format!("{}{PART_SUFFIX}", request.file_name));
    Ok((destination, part))
}

fn validate_file_name(name: &str) -> Result<(), BackendError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0', ':'])
        || name.ends_with(PART_SUFFIX);
    if bad {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "invalid destination file name",
        ));
    }
    Ok(())
}

impl DownloadBackend for HttpBackend {
    fn id(&self) -> &'static str {
        "http"
    }

    fn capabilities(&self) -> Vec<CapabilityReport> {
        capabilities::report()
    }

    async fn start(&self, request: TransferRequest) -> Result<BackendHandle, BackendError> {
        let (destination, part) = prepare(&request, &self.config, &self.attempts)?;
        let key = request.attempt_id.as_str().to_owned();
        transfer::create_part(&destination, &part)?;
        let attempt = Arc::new(Attempt::new(
            request,
            part,
            Shared {
                state: TransferState::Waiting,
                completed: 0,
                total: None,
            },
        ));
        lock(&self.attempts).insert(key.clone(), Arc::clone(&attempt));
        self.spawn(&attempt);
        Ok(BackendHandle(key))
    }

    async fn pause(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle)?;
        stop(&attempt, TransferState::Paused).await;
        Ok(())
    }

    async fn resume(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle)?;
        let (paused, completed) = {
            let shared = lock(&attempt.shared);
            (shared.state == TransferState::Paused, shared.completed)
        };
        if !paused {
            return Ok(());
        }
        let seg_checkpoint = lock(&attempt.seg_checkpoint).clone();
        if let Some(cp) = seg_checkpoint {
            if cp.confirmed() == 0 {
                // Nothing confirmed: start over (a fresh probe decides the plan again).
                transfer::truncate_part(&attempt.part)?;
                *lock(&attempt.seg_checkpoint) = None;
                *lock(&attempt.shared) = Shared {
                    state: TransferState::Waiting,
                    completed: 0,
                    total: None,
                };
            } else {
                segmented::verify(&attempt.part, &cp).await?;
                lock(&attempt.shared).state = TransferState::Waiting;
                *lock(&attempt.seg_resume_from) = Some(cp);
            }
            self.spawn(&attempt);
            return Ok(());
        }
        let checkpoint = lock(&attempt.checkpoint).clone();
        match checkpoint {
            // Nothing on disk yet: a plain restart.
            None if completed == 0 => {
                transfer::truncate_part(&attempt.part)?;
                *lock(&attempt.shared) = Shared {
                    state: TransferState::Waiting,
                    completed: 0,
                    total: None,
                };
            }
            Some(cp) if cp.offset == 0 => {
                transfer::truncate_part(&attempt.part)?;
                *lock(&attempt.checkpoint) = None;
                *lock(&attempt.shared) = Shared {
                    state: TransferState::Waiting,
                    completed: 0,
                    total: None,
                };
            }
            None => {
                return Err(BackendError::new(
                    ErrorKind::BackendCapabilityUnavailable,
                    "no confirmed resume point for this transfer",
                ));
            }
            Some(cp) => {
                cp.check_resumable()?;
                transfer::verify_partial(&attempt.part, &cp).await?;
                lock(&attempt.shared).state = TransferState::Waiting;
                *lock(&attempt.resume_from) = Some(cp);
            }
        }
        self.spawn(&attempt);
        Ok(())
    }

    async fn remove(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle)?;
        stop(&attempt, TransferState::Removed).await;
        lock(&self.attempts).remove(&handle.0);
        Ok(())
    }

    async fn discard(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle)?;
        stop(&attempt, TransferState::Removed).await;
        if matches!(lock(&attempt.shared).state, TransferState::Complete) {
            return Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "a completed transfer is never discarded",
            ));
        }
        // Only the attempt's own partial file; never the destination.
        match std::fs::remove_file(&attempt.part) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(classify::io_error("cannot delete partial file", &e)),
        }
        lock(&self.attempts).remove(&handle.0);
        Ok(())
    }

    async fn staged_output(&self, handle: &BackendHandle) -> Result<StagedOutput, BackendError> {
        let attempt = self.attempt(handle)?;
        if lock(&attempt.shared).state != TransferState::Complete {
            return Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "the transfer is not complete",
            ));
        }
        Ok(StagedOutput::Staged(attempt.part.clone()))
    }

    async fn checkpoint(
        &self,
        handle: &BackendHandle,
    ) -> Result<Option<DurableCheckpoint>, BackendError> {
        let attempt = self.attempt(handle)?;
        if !matches!(
            lock(&attempt.shared).state,
            TransferState::Waiting | TransferState::Active | TransferState::Paused
        ) {
            return Ok(None);
        }
        let (dir, name) = (
            &attempt.request.destination_dir,
            attempt.request.file_name.as_str(),
        );
        // Both are published only after the bytes they describe were flushed and synced.
        let segmented = lock(&attempt.seg_checkpoint).clone();
        if let Some(cp) = segmented {
            return durable::export_segmented(&cp, dir, name);
        }
        let single = lock(&attempt.checkpoint).clone();
        match single {
            Some(cp) => durable::export_single(&cp, dir, name),
            None => Ok(None),
        }
    }

    async fn restore(
        &self,
        request: TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> Result<BackendHandle, BackendError> {
        let (destination, part) = prepare(&request, &self.config, &self.attempts)?;
        if destination.exists() {
            return Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "the target file already exists; the interrupted download was not continued over it",
            ));
        }
        let restored = durable::import(
            checkpoint,
            &request.destination_dir,
            &request.file_name,
            &part,
        )
        .await?;
        let url = match &restored {
            durable::Restored::Single(cp) => &cp.identity.url,
            durable::Restored::Segmented(cp) => &cp.identity.url,
        };
        transfer::gate(url, &self.config.scheme_gate)?;
        let key = request.attempt_id.as_str().to_owned();
        let attempt = Attempt::new(
            request,
            part,
            Shared {
                state: TransferState::Paused,
                completed: restored.confirmed(),
                total: restored.length(),
            },
        );
        match restored {
            durable::Restored::Single(cp) => *lock(&attempt.checkpoint) = Some(cp),
            durable::Restored::Segmented(cp) => *lock(&attempt.seg_checkpoint) = Some(cp),
        }
        lock(&self.attempts).insert(key.clone(), Arc::new(attempt));
        Ok(BackendHandle(key))
    }

    async fn progress(&self, handle: &BackendHandle) -> Result<BackendProgress, BackendError> {
        let attempt = self.attempt(handle)?;
        let shared = lock(&attempt.shared);
        Ok(BackendProgress {
            state: shared.state.clone(),
            completed: ByteCount(shared.completed),
            total: shared.total.map(ByteCount),
            connections: u32::from(matches!(shared.state, TransferState::Active)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_validated() {
        for bad in ["", ".", "..", "a/b", "a\\b", "c:x", "x.bin.downova-part"] {
            assert!(validate_file_name(bad).is_err(), "{bad}");
        }
        assert!(validate_file_name("ok file (1).zip").is_ok());
    }
}
