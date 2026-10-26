//! Validated segmented transfers (B0.4.3).
//!
//! A transfer is split only when the probe `200` established a strong ETag and a known length,
//! the request allows more than one connection, and every segment would be at least
//! [`MIN_SEGMENT`] bytes. Segments are contiguous, non-overlapping and cover `0..length` exactly.
//!
//! Each worker owns its file handle (no shared cursor) and a bounded buffer, and sends
//! `Range: bytes=<first>-<last>` + `If-Range: <ETag>` + `Accept-Encoding: identity`. A `206` is
//! written only after [`validate_range_response`] accepts it (exact range, same ETag and total,
//! identity encoding, matching `Content-Length`). Anything else halts every worker; the
//! orchestrator waits for all of them before deciding:
//! - external stop (pause/remove) → checkpoint of confirmed bytes per segment, `Stopped`;
//! - a `200` in any segment → `Restart` (the whole file restarts on one connection);
//! - any other failure → the failure, nothing published;
//! - all segments complete → `sync_all`, then the caller publishes.

use std::path::Path;
use std::sync::Arc;

use dm_backend_api::{BackendError, TransferState};
use dm_domain::ErrorKind;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, IF_RANGE, RANGE};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use crate::classify::{io_error, transport_error};
use crate::transfer::{
    Ended, Identity, SYNC_BYTES, SYNC_EVERY, gate, gate_url, status_failure,
    validate_range_response, with_caller_headers,
};
use crate::{Attempt, HttpConfig, lock};

/// Smallest segment worth a separate connection.
pub(crate) const MIN_SEGMENT: u64 = 1024 * 1024;
/// Upper bound on connections per file, whatever the request allows (research §15.4).
pub(crate) const MAX_SEGMENTS: u8 = 4;
/// Write buffer per worker: memory is bounded by `MAX_SEGMENTS * SEGMENT_BUFFER`.
const SEGMENT_BUFFER: usize = 128 * 1024;

/// One segment: `start..end` of the file, `done` bytes confirmed from `start`, and the SHA-256
/// state of exactly those bytes.
#[derive(Clone)]
pub(crate) struct Segment {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) done: u64,
    pub(crate) hasher: Sha256,
}

impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{} done {}", self.start, self.end, self.done)
    }
}

impl Segment {
    fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    fn complete(&self) -> bool {
        self.done == self.len()
    }
}

/// Confirmed state of a segmented transfer.
#[derive(Debug, Clone)]
pub(crate) struct SegCheckpoint {
    pub(crate) identity: Identity,
    pub(crate) length: u64,
    pub(crate) segments: Vec<Segment>,
}

impl SegCheckpoint {
    pub(crate) fn confirmed(&self) -> u64 {
        self.segments
            .iter()
            .fold(0u64, |acc, s| acc.saturating_add(s.done))
    }
}

/// Splits `length` into `n` contiguous ranges (the first `length % n` one byte longer).
pub(crate) fn split(length: u64, n: u64) -> Option<Vec<(u64, u64)>> {
    if n == 0 || length == 0 {
        return None;
    }
    let base = length.checked_div(n)?;
    let extra = length.checked_rem(n)?;
    let mut ranges = Vec::new();
    let mut start: u64 = 0;
    for i in 0..n {
        let size = base.checked_add(u64::from(i < extra))?;
        let end = start.checked_add(size)?;
        ranges.push((start, end));
        start = end;
    }
    (start == length).then_some(ranges)
}

/// The segment plan for a probed identity, or `None` for the single-connection path.
pub(crate) fn plan(identity: &Identity, max_connections: u8) -> Option<SegCheckpoint> {
    identity.etag.as_ref()?;
    let length = identity.length?;
    let by_size = length.checked_div(MIN_SEGMENT)?;
    let n = u64::from(max_connections.min(MAX_SEGMENTS)).min(by_size);
    if n < 2 {
        return None;
    }
    let segments = split(length, n)?
        .into_iter()
        .map(|(start, end)| Segment {
            start,
            end,
            done: 0,
            hasher: Sha256::new(),
        })
        .collect();
    Some(SegCheckpoint {
        identity: identity.clone(),
        length,
        segments,
    })
}

/// Re-reads every segment's confirmed bytes and compares them with the checkpoint.
pub(crate) async fn verify(part: &Path, cp: &SegCheckpoint) -> Result<(), BackendError> {
    let part = part.to_owned();
    let cp = cp.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom};
        let changed = |what: &str| {
            BackendError::new(
                ErrorKind::InvalidRange,
                format!("the partial file was {what} outside Downova; the download must restart"),
            )
        };
        let mut file = match std::fs::File::open(&part) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(changed("deleted")),
            Err(e) => return Err(io_error("cannot read partial file", &e)),
        };
        let len = file
            .metadata()
            .map_err(|e| io_error("cannot read partial file", &e))?
            .len();
        if len != cp.length {
            return Err(changed(if len < cp.length {
                "truncated"
            } else {
                "extended"
            }));
        }
        let mut buf = vec![0u8; SEGMENT_BUFFER];
        for seg in &cp.segments {
            file.seek(SeekFrom::Start(seg.start))
                .map_err(|e| io_error("cannot read partial file", &e))?;
            let mut hasher = Sha256::new();
            let mut left = seg.done;
            while left > 0 {
                let want =
                    usize::try_from(left.min(SEGMENT_BUFFER as u64)).unwrap_or(SEGMENT_BUFFER);
                let chunk = buf.get_mut(..want).unwrap_or_default();
                file.read_exact(chunk).map_err(|_| changed("truncated"))?;
                hasher.update(&*chunk);
                left = left.saturating_sub(want as u64);
            }
            if hasher.finalize() != seg.hasher.clone().finalize() {
                return Err(changed("modified"));
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| BackendError::new(ErrorKind::Internal, "partial file check failed"))?
}

/// How one worker ended.
enum WorkerEnd {
    Done,
    Stopped,
    Restart,
    Failed(BackendError),
}

/// Resolves `fut` unless the external stop or the internal halt fires first.
async fn or_halt<T>(
    stop: &mut watch::Receiver<bool>,
    halt: &mut watch::Receiver<bool>,
    fut: impl std::future::Future<Output = T>,
) -> Option<T> {
    if *stop.borrow() || *halt.borrow() {
        return None;
    }
    tokio::select! {
        biased;
        _ = stop.changed() => None,
        _ = halt.changed() => None,
        value = fut => Some(value),
    }
}

#[allow(clippy::too_many_arguments)]
async fn worker(
    client: reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    identity: &Identity,
    length: u64,
    idx: usize,
    mut seg: Segment,
    mut stop: watch::Receiver<bool>,
    mut halt: watch::Receiver<bool>,
) -> (Segment, WorkerEnd) {
    if seg.complete() {
        return (seg, WorkerEnd::Done);
    }
    let fail = |seg: Segment, e: BackendError| (seg, WorkerEnd::Failed(e));
    let (Some(first), Some(last)) = (seg.start.checked_add(seg.done), seg.end.checked_sub(1))
    else {
        return fail(
            seg,
            BackendError::new(ErrorKind::Internal, "segment bounds"),
        );
    };
    let Some(etag) = identity.etag.clone() else {
        return fail(
            seg,
            BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "no strong validator",
            ),
        );
    };
    if let Err(e) = gate(&identity.url, &config.scheme_gate) {
        return fail(seg, e);
    }
    let origin = match gate_url(&attempt.request.url, &config.scheme_gate) {
        Ok(url) => url.origin(),
        Err(e) => return fail(seg, e),
    };
    let req = client
        .get(identity.url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={first}-{last}"))
        .header(IF_RANGE, etag.clone());
    let req = with_caller_headers(req, attempt, identity.url.origin() == origin);
    let response = match or_halt(&mut stop, &mut halt, req.send()).await {
        None => return (seg, WorkerEnd::Stopped),
        Some(Err(e)) => return fail(seg, transport_error(&e)),
        Some(Ok(response)) => response,
    };
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {}
        // Validator mismatch or ranges unsupported: never splice it.
        reqwest::StatusCode::OK => return (seg, WorkerEnd::Restart),
        s if s.is_redirection() => {
            return fail(
                seg,
                BackendError::new(
                    ErrorKind::RemoteChanged,
                    "the file moved since the download started; it must restart",
                ),
            );
        }
        _ => return fail(seg, status_failure(&response)),
    }
    if let Err(e) = validate_range_response(response.headers(), Some(&etag), length, first, last) {
        return fail(seg, e);
    }

    let file = match tokio::fs::OpenOptions::new()
        .write(true)
        .open(&attempt.part)
        .await
    {
        Ok(file) => file,
        Err(e) => return fail(seg, io_error("cannot open partial file", &e)),
    };
    let mut file = file;
    if let Err(e) = file.seek(std::io::SeekFrom::Start(first)).await {
        return fail(seg, io_error("cannot position partial file", &e));
    }
    let mut out = tokio::io::BufWriter::with_capacity(SEGMENT_BUFFER, file);
    let mut body = response.bytes_stream();
    // Bytes in `out` but not yet flushed are counted in `pending` and confirmed on flush.
    let mut pending_hasher = seg.hasher.clone();
    let mut pending: u64 = 0;
    let mut last_sync = tokio::time::Instant::now();
    let end = loop {
        let next = match or_halt(&mut stop, &mut halt, body.next()).await {
            None => break WorkerEnd::Stopped,
            Some(next) => next,
        };
        let chunk = match next {
            None => break WorkerEnd::Done,
            Some(Err(e)) => break WorkerEnd::Failed(transport_error(&e)),
            Some(Ok(chunk)) => chunk,
        };
        let n = chunk.len() as u64;
        let total_done = seg.done.checked_add(pending).and_then(|d| d.checked_add(n));
        if total_done.is_none_or(|d| d > seg.len()) {
            break WorkerEnd::Failed(BackendError::new(
                ErrorKind::InvalidRange,
                "the server sent more bytes than the segment holds",
            ));
        }
        if let Err(e) = out.write_all(&chunk).await {
            break WorkerEnd::Failed(io_error("cannot write partial file", &e));
        }
        pending_hasher.update(&chunk);
        pending = pending.saturating_add(n);
        {
            let mut shared = lock(&attempt.shared);
            shared.completed = shared.completed.saturating_add(n);
        }
        if pending >= SYNC_BYTES || last_sync.elapsed() >= SYNC_EVERY {
            // Durable progress: flush and sync, then publish this segment's confirmed state.
            if let Err(e) = out.flush().await {
                break WorkerEnd::Failed(io_error("cannot write partial file", &e));
            }
            if let Err(e) = out.get_ref().sync_data().await {
                break WorkerEnd::Failed(io_error("cannot flush partial file", &e));
            }
            seg.done = seg.done.saturating_add(pending);
            seg.hasher = pending_hasher.clone();
            pending = 0;
            if let Some(slot) = lock(&attempt.seg_checkpoint)
                .as_mut()
                .and_then(|cp| cp.segments.get_mut(idx))
            {
                *slot = seg.clone();
            }
            last_sync = tokio::time::Instant::now();
        }
    };
    // Confirm what reached the file (flushed and synced). Validated bytes are kept on every path.
    if out.flush().await.is_ok() && out.get_ref().sync_data().await.is_ok() {
        seg.done = seg.done.saturating_add(pending);
        seg.hasher = pending_hasher;
    } else {
        // Unconfirmed bytes are not counted: the progress drops back to what is on disk.
        let mut shared = lock(&attempt.shared);
        shared.completed = shared.completed.saturating_sub(pending);
    }
    let end = match end {
        WorkerEnd::Done if !seg.complete() => WorkerEnd::Failed(BackendError::new(
            ErrorKind::NetworkOffline,
            "a segment ended before all of its bytes arrived",
        )),
        other => other,
    };
    (seg, end)
}

/// Runs (or resumes) a segmented transfer. See the module documentation for the outcomes.
pub(crate) async fn run(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    mut cp: SegCheckpoint,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let fresh = cp.confirmed() == 0 && cp.segments.iter().all(|s| s.done == 0);
    {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&attempt.part)
            .map_err(|e| io_error("cannot open partial file", &e))?;
        if fresh {
            // Size the file once; workers write into their own regions.
            file.set_len(cp.length)
                .map_err(|e| io_error("cannot size partial file", &e))?;
        } else {
            let len = file
                .metadata()
                .map_err(|e| io_error("cannot read partial file", &e))?
                .len();
            if len != cp.length {
                return Err(BackendError::new(
                    ErrorKind::InvalidRange,
                    "the partial file changed before resuming; nothing was written",
                )
                .into());
            }
        }
    }
    {
        let mut shared = lock(&attempt.shared);
        shared.state = TransferState::Active;
        shared.total = Some(cp.length);
        shared.completed = cp.confirmed();
    }
    *lock(&attempt.seg_checkpoint) = Some(cp.clone());

    let (halt_tx, halt_rx) = watch::channel(false);
    let halt_tx = Arc::new(halt_tx);
    let workers = cp.segments.iter().cloned().enumerate().map(|(idx, seg)| {
        let (client, stop, halt, halt_tx) = (
            client.clone(),
            stop.clone(),
            halt_rx.clone(),
            Arc::clone(&halt_tx),
        );
        let identity = cp.identity.clone();
        let length = cp.length;
        async move {
            let (seg, end) = worker(
                client, config, attempt, &identity, length, idx, seg, stop, halt,
            )
            .await;
            if matches!(end, WorkerEnd::Failed(_) | WorkerEnd::Restart) {
                let _ = halt_tx.send(true);
            }
            (seg, end)
        }
    });
    // Every worker has finished (and flushed) before anything is decided.
    let results = futures_util::future::join_all(workers).await;

    let mut failure: Option<BackendError> = None;
    let mut restart = false;
    let mut segments = Vec::with_capacity(results.len());
    for (seg, end) in results {
        match end {
            WorkerEnd::Failed(e) if failure.is_none() => failure = Some(e),
            WorkerEnd::Restart => restart = true,
            _ => {}
        }
        segments.push(seg);
    }
    cp.segments = segments;
    lock(&attempt.shared).completed = cp.confirmed();
    *lock(&attempt.seg_checkpoint) = Some(cp.clone());

    if *stop.borrow() {
        return Err(Ended::Stopped);
    }
    if restart {
        return Err(Ended::Restart);
    }
    if let Some(e) = failure {
        return Err(Ended::Failed(e));
    }
    if !cp.segments.iter().all(Segment::complete) {
        return Err(BackendError::new(ErrorKind::Internal, "segments incomplete").into());
    }
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&attempt.part)
        .await
        .map_err(|e| io_error("cannot open partial file", &e))?;
    file.sync_all()
        .await
        .map_err(|e| io_error("cannot flush partial file", &e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;

    fn identity(etag: Option<&str>, length: Option<u64>) -> Identity {
        Identity {
            etag: etag.map(str::to_owned),
            length,
            url: Url::parse("http://h/f").unwrap(),
        }
    }

    #[test]
    fn split_covers_exactly_without_overlap() {
        for (len, n) in [
            (10u64, 3u64),
            (4 * MIN_SEGMENT + 1, 4),
            (7, 7),
            (u64::MAX, 4),
        ] {
            let r = split(len, n).unwrap();
            assert_eq!(r.len() as u64, n);
            assert_eq!(r[0].0, 0);
            assert_eq!(r.last().unwrap().1, len);
            for w in r.windows(2) {
                assert_eq!(w[0].1, w[1].0, "contiguous");
                assert!(w[0].0 < w[0].1);
            }
        }
        assert!(split(0, 4).is_none());
        assert!(split(10, 0).is_none());
    }

    #[test]
    fn plan_needs_a_strong_etag_a_length_and_enough_bytes() {
        let big = Some(8 * MIN_SEGMENT);
        assert_eq!(
            plan(&identity(Some("\"v\""), big), 4)
                .unwrap()
                .segments
                .len(),
            4
        );
        assert_eq!(
            plan(&identity(Some("\"v\""), big), 8)
                .unwrap()
                .segments
                .len(),
            4,
            "capped"
        );
        assert_eq!(
            plan(&identity(Some("\"v\""), big), 2)
                .unwrap()
                .segments
                .len(),
            2
        );
        assert!(
            plan(&identity(Some("\"v\""), big), 1).is_none(),
            "single requested"
        );
        assert!(plan(&identity(None, big), 4).is_none(), "no strong ETag");
        assert!(
            plan(&identity(Some("\"v\""), None), 4).is_none(),
            "no length"
        );
        assert!(
            plan(&identity(Some("\"v\""), Some(MIN_SEGMENT * 2 - 1)), 4).is_none(),
            "small file"
        );
        assert_eq!(
            plan(&identity(Some("\"v\""), Some(3 * MIN_SEGMENT)), 4)
                .unwrap()
                .segments
                .len(),
            3,
            "never below the minimum segment size"
        );
    }
}
