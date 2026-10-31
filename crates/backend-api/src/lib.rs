//! Replaceable download-backend boundary (ADR 0002).
//!
//! The service talks to a transfer engine only through [`DownloadBackend`]. Engine adapters
//! (`dm-backend-aria2`, a future custom engine) implement it. This crate sits between them so
//! neither depends on the other: `dm-service → dm-backend-api ← dm-backend-aria2`.
//!
//! Backend handles (for aria2, the GID) are opaque [`BackendHandle`]s owned by the service's
//! attempt mapping; they are never exposed as job IDs, persisted as the job identity, or shown in
//! the UI. Public APIs never pass arbitrary backend options through.

use std::fmt;
use std::future::Future;
use std::path::PathBuf;

use dm_domain::capability::support_of;
use dm_domain::redact::Secret;
use dm_domain::{AttemptId, ByteCount, Capability, CapabilityReport, CapabilitySupport, ErrorKind};

/// Opaque backend-specific transfer handle (e.g. an aria2 GID). Scoped to one attempt.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendHandle(pub String);

/// A validated request to start one transfer attempt.
#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub attempt_id: AttemptId,
    /// Raw source URL; may contain secrets. Never log without `redact_url`.
    pub url: String,
    /// Credentials resolved from the OS credential store for this attempt only.
    pub headers: Vec<(String, Secret)>,
    pub destination_dir: PathBuf,
    /// The job's staging name, chosen by the service and stable for the job's lifetime (B1.6):
    /// partial and control files are named after it. It is not the final name, which the service
    /// chooses later and applies only when it places the finished file.
    pub file_name: String,
    /// Upper bound of connections for this file; the backend may use fewer.
    pub max_connections: u8,
}

/// Transfer state as reported by the engine. `Complete` means the engine finished writing and
/// synced its output (see [`StagedOutput`]); it is not the job's `completed` phase, which also
/// needs verification and finalization by the service (ADV06, ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferState {
    Waiting,
    Active,
    Paused,
    Complete,
    Removed,
    Failed(BackendError),
}

/// Live, non-durable progress reported by a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendProgress {
    pub state: TransferState,
    pub completed: ByteCount,
    /// `None` when the engine does not know the total yet.
    pub total: Option<ByteCount>,
    pub connections: u32,
    /// Naming metadata of the attempt's latest successful response, once one arrived (B1.6).
    /// `None` when the engine cannot see response headers (aria2) or no response arrived yet.
    pub naming: Option<ResponseNaming>,
}

/// Advisory naming inputs from a response (B1.6, ADR 0011). The backend reports them unparsed;
/// the service parses and sanitizes them and chooses the final name and its collisions. Nothing
/// here ever selects a directory or renames a partial file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponseNaming {
    /// The `Content-Disposition` field value of the final response after redirects (a `200` that
    /// starts a full body), at most [`MAX_DISPOSITION_BYTES`]. Bytes that are not UTF-8 are read
    /// as ISO-8859-1, as HTTP defines field values.
    pub content_disposition: Option<String>,
    /// The last path segment of that response's URL, still percent-encoded. Never the query or
    /// fragment, which may hold tokens.
    pub url_file_name: Option<String>,
}

/// Longest `Content-Disposition` value a backend reports; a longer one is dropped whole.
pub const MAX_DISPOSITION_BYTES: usize = 4096;

/// Backend failure, already classified into a stable [`ErrorKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub kind: ErrorKind,
    /// Sanitized detail; must not contain URLs with tokens, cookies or headers.
    pub detail: String,
    /// Server-requested wait (`Retry-After`), when the engine can observe it. The service owns
    /// the retry decision (COR-04); engines that cannot see response headers leave this `None`.
    pub retry_after: Option<std::time::Duration>,
}

impl BackendError {
    /// Creates an error from a kind and a sanitized detail.
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            retry_after: None,
        }
    }

    /// Attaches a server-requested wait.
    pub fn with_retry_after(mut self, wait: std::time::Duration) -> Self {
        self.retry_after = Some(wait);
        self
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for BackendError {}

/// Disk budget check supplied by the service (B1.4, ADR 0009). Engines call it before they
/// consume space; the service builds it from the platform's free-space probe, so adapters never
/// depend on `dm-platform`.
///
/// The check is **advisory**: it compares a free-space snapshot with what the caller still needs
/// plus the guard's safety margin. It reserves nothing, so concurrent writers on the same volume can
/// still run it out; a real `ENOSPC` remains possible and is classified `DISK_FULL`.
pub trait SpaceGuard: Send + Sync + fmt::Debug {
    /// `Ok(())` when at least `needed` more bytes (plus the margin) appear to be free in `dir`.
    /// `DISK_FULL` or `VOLUME_MISSING` otherwise. When free space cannot be determined the guard
    /// returns `Ok(())` and says so in its own diagnostics: an unknown value is never invented.
    fn ensure(&self, dir: &std::path::Path, needed: u64) -> Result<(), BackendError>;
}

/// The guard used when none is configured: every check passes (B0 behaviour).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSpaceGuard;

impl SpaceGuard for NoSpaceGuard {
    fn ensure(&self, _dir: &std::path::Path, _needed: u64) -> Result<(), BackendError> {
        Ok(())
    }
}

/// Where a finished transfer's bytes are (B1.3, ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedOutput {
    /// A complete, synced staging file next to the destination (same directory, so the same
    /// volume), owned by the backend's attempt. Nothing is at the destination yet: the service
    /// verifies the file and places it without overwriting anything.
    Staged(std::path::PathBuf),
    /// The engine itself wrote the destination (aria2). Verification happens in place, after the
    /// file is already visible; a failed check cannot un-place it.
    PlacedByEngine(std::path::PathBuf),
}

/// Upper bound on an encoded [`DurableCheckpoint`] (B1.2, ADR 0007).
pub const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;

/// A durable resume checkpoint, exported by a backend and stored by the service (ADR 0007).
///
/// The service treats `payload` as opaque: it stores it next to the job and attempt it belongs to
/// and hands it back to the same backend after a restart. Segment layout, validators and digests
/// are encoded and interpreted only by the backend. A backend exports a checkpoint only for bytes
/// it has flushed and synced to the partial file, so the service may commit it immediately.
#[derive(Clone, PartialEq, Eq)]
pub struct DurableCheckpoint {
    /// [`DownloadBackend::id`] of the backend that wrote it.
    pub backend: String,
    /// Backend-owned payload format version.
    pub format_version: u32,
    /// Bytes confirmed on disk by this checkpoint (for progress and tests; not authoritative).
    pub confirmed: ByteCount,
    /// Backend-owned encoding, at most [`MAX_CHECKPOINT_BYTES`]. May contain the source URL, so it
    /// is never logged or exported.
    pub payload: Vec<u8>,
}

impl fmt::Debug for DurableCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DurableCheckpoint")
            .field("backend", &self.backend)
            .field("format_version", &self.format_version)
            .field("confirmed", &self.confirmed)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

/// A transfer engine behind the service.
/// Outcome of [`DownloadBackend::verify_replacement`] (B2.17, ADR 0020).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacementCheck {
    /// The replacement serves the representation the checkpoint describes: the same strong ETag
    /// and length, a validated `206` for the sampled confirmed bytes, and those bytes equal the
    /// ones on disk. `checkpoint` is the same progress bound to the replacement's final URL.
    Compatible {
        checkpoint: DurableCheckpoint,
        /// The final URL has the same origin and path as the original's (only the query, for
        /// example a signature, differs), so the ETag is scoped to the same resource.
        same_resource: bool,
    },
    /// Not provably the same content; nothing was changed.
    Incompatible(dm_domain::api::ReplaceReason),
}

pub trait DownloadBackend: Send + Sync {
    /// Stable backend identifier, e.g. `"aria2"`.
    fn id(&self) -> &'static str;

    /// Complete capability report. Capabilities not proven by fixtures must be `Unverified`.
    fn capabilities(&self) -> Vec<CapabilityReport>;

    /// Starts one attempt and returns the backend's handle for it.
    fn start(
        &self,
        request: TransferRequest,
    ) -> impl Future<Output = Result<BackendHandle, BackendError>> + Send;

    /// Pauses a transfer, keeping engine-owned resume state.
    fn pause(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Resumes a paused transfer.
    fn resume(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Removes a transfer. After this returns, the backend reports no further progress for it.
    fn remove(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Removes a stopped transfer **and deletes its partial output** (file and any engine resume
    /// state), so that a fresh attempt for the same destination starts from zero instead of an
    /// unvalidated resume. Used before automatic retries while strong-validator resume is not
    /// supported. Never called for a completed transfer.
    fn discard(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// Reads live progress (not a durability checkpoint).
    fn progress(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<BackendProgress, BackendError>> + Send;

    /// The output of a transfer that reported [`TransferState::Complete`]. An error for any other
    /// state. The backend never moves the file afterwards; after placement the service calls
    /// `remove` to forget the attempt (which deletes nothing).
    fn staged_output(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<StagedOutput, BackendError>> + Send;

    /// Exports the attempt's latest durable checkpoint: only bytes already flushed and synced to
    /// the partial file. `Ok(None)` when nothing resumable is confirmed (or the backend does not
    /// support [`Capability::DurableResume`]).
    fn checkpoint(
        &self,
        handle: &BackendHandle,
    ) -> impl Future<Output = Result<Option<DurableCheckpoint>, BackendError>> + Send;

    /// Recreates an attempt from a stored checkpoint after a service restart. The backend checks
    /// that the checkpoint is its own, compatible and bound to `request`'s destination, and
    /// re-verifies the confirmed data on disk. The new attempt is **paused**; `resume` continues it
    /// through the normal validated path. Any mismatch is an error; nothing is guessed.
    fn restore(
        &self,
        request: TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> impl Future<Output = Result<BackendHandle, BackendError>> + Send;

    /// Deletes the partial output and resume state this backend keeps for `request`'s destination
    /// and file name when **no attempt holds them** (an explicit Retry or Refresh-link restart of a
    /// job whose attempt ended with an earlier service run). Never touches the final file. Missing
    /// output is not an error. Backends that cannot identify their own leftovers refuse.
    fn discard_output(
        &self,
        request: &TransferRequest,
    ) -> impl Future<Output = Result<(), BackendError>> + Send {
        let _ = request;
        async {
            Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "this download engine cannot release leftover partial data",
            ))
        }
    }

    /// Checks whether `request.url` (a replacement link) serves exactly the content `checkpoint`
    /// describes, without changing any state (B2.17, ADR 0020). Backends that cannot prove it
    /// answer `Incompatible(Unverifiable)`.
    fn verify_replacement(
        &self,
        request: &TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> impl Future<Output = Result<ReplacementCheck, BackendError>> + Send {
        let _ = (request, checkpoint);
        async {
            Ok(ReplacementCheck::Incompatible(
                dm_domain::api::ReplaceReason::Unverifiable,
            ))
        }
    }
}

/// The file-system refusals every adapter and the service report as `DESTINATION_UNWRITABLE`
/// (B1.6), with the reason shown to the user (never the path).
pub fn destination_refused(kind: std::io::ErrorKind) -> Option<&'static str> {
    use std::io::ErrorKind as K;
    match kind {
        // ENAMETOOLONG, and Windows ERROR_INVALID_NAME / ERROR_FILENAME_EXCED_RANGE.
        K::InvalidFilename => {
            Some("the file system does not accept this file name (too long or invalid there)")
        }
        K::PermissionDenied => Some("no permission to write in the download folder"),
        K::ReadOnlyFilesystem => Some("the download folder is on a read-only volume"),
        K::NotADirectory | K::IsADirectory => Some("the download path is not a usable folder"),
        _ => None,
    }
}

/// Gates a feature on an explicitly supported capability.
///
/// `Unsupported`, `Unverified` and absent capabilities are refused with
/// `BACKEND_CAPABILITY_UNAVAILABLE`; there is never a silent fallback (for example, an
/// unsupported proxy profile must not turn into a direct connection).
pub fn require_capability(
    report: &[CapabilityReport],
    capability: Capability,
) -> Result<(), BackendError> {
    match support_of(report, capability) {
        CapabilitySupport::Supported => Ok(()),
        other => Err(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            format!("{capability:?} is {other:?} for this backend"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(capability: Capability, support: CapabilitySupport) -> CapabilityReport {
        CapabilityReport {
            capability,
            support,
            note: None,
        }
    }

    #[test]
    fn only_supported_capabilities_pass() {
        let caps = vec![
            report(Capability::Http, CapabilitySupport::Supported),
            report(Capability::Http2, CapabilitySupport::Unverified),
            report(Capability::Socks5Proxy, CapabilitySupport::Unsupported),
        ];
        assert!(require_capability(&caps, Capability::Http).is_ok());
        for refused in [
            Capability::Http2,
            Capability::Socks5Proxy,
            Capability::PacProxy,
        ] {
            let err = require_capability(&caps, refused).unwrap_err();
            assert_eq!(err.kind, ErrorKind::BackendCapabilityUnavailable);
        }
    }
}
