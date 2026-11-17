//! Explicit single-item media backend replacing the optional aria2 adapter (ADR 0042).
//!
//! Each attempt owns one yt-dlp child and a private staging directory on the destination volume.
//! URLs go through stdin, never process arguments. Ambient config, plug-ins, external downloaders,
//! playlists, live streams, postprocessors and unvalidated continuation are disabled. This first
//! integration selects one already combined format, without invoking FFmpeg. The service still
//! owns queues, verification, final names, collision policy and public job identities.

pub mod binary;

use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, DurableCheckpoint, NoSpaceGuard,
    ResponseNaming, SpaceGuard, StagedOutput, TransferRequest, TransferState,
};
use dm_domain::{ByteCount, Capability, CapabilityReport, CapabilitySupport, ErrorKind};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

pub use binary::YtdlpBinary;

/// Conservative sidecar admission limit, independent of the native engine's shared host budget.
pub const MAX_ACTIVE: usize = 3;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub binary: YtdlpBinary,
    pub space: Arc<dyn SpaceGuard>,
    /// An additional test/deployment CA bundle. Certificate verification always remains enabled.
    pub ca_file: Option<PathBuf>,
}

impl LaunchConfig {
    pub fn new(binary: YtdlpBinary) -> Self {
        Self {
            binary,
            space: Arc::new(NoSpaceGuard),
            ca_file: None,
        }
    }
}

#[derive(Debug)]
struct Stop {
    state: TransferState,
    ack: oneshot::Sender<()>,
}

struct Attempt {
    request: TransferRequest,
    work: PathBuf,
    progress: Arc<Mutex<BackendProgress>>,
    control: mpsc::Sender<Stop>,
    done: watch::Receiver<bool>,
}

impl fmt::Debug for Attempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TransferRequest.url can contain tokens: never include the request in diagnostics.
        f.debug_struct("Attempt").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct YtdlpBackend {
    config: LaunchConfig,
    attempts: tokio::sync::Mutex<HashMap<String, Arc<Attempt>>>,
}

impl YtdlpBackend {
    pub fn new(config: LaunchConfig) -> Self {
        Self {
            config,
            attempts: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn version(&self) -> &str {
        &self.config.binary.version
    }

    pub async fn shutdown(&self) -> Result<(), BackendError> {
        let attempts: Vec<_> = self.attempts.lock().await.values().cloned().collect();
        for attempt in attempts {
            stop(&attempt, TransferState::Paused).await?;
        }
        Ok(())
    }

    async fn attempt(&self, handle: &BackendHandle) -> Result<Arc<Attempt>, BackendError> {
        self.attempts
            .lock()
            .await
            .get(&handle.0)
            .cloned()
            .ok_or_else(|| error(ErrorKind::InvalidRequest, "unknown yt-dlp attempt"))
    }
}

/// No upstream feature is implicitly inherited. Only this integration's offline fixtures establish
/// public HTTP(S) single-item media transfer. In particular, no browser credentials or partial
/// resume state cross this adapter, and proxy/speed settings are never silently ignored.
pub fn capabilities() -> Vec<CapabilityReport> {
    Capability::ALL.into_iter().map(|capability| CapabilityReport {
        capability,
        support: if matches!(capability, Capability::Http | Capability::Https) {
            CapabilitySupport::Supported
        } else { CapabilitySupport::Unsupported },
        note: Some(if matches!(capability, Capability::Http | Capability::Https) {
            "Single public media item, one already combined format; site availability depends on yt-dlp."
        } else {
            "Not integrated by the yt-dlp adapter; use the native engine for ordinary files."
        }.to_owned()),
    }).collect()
}

impl DownloadBackend for YtdlpBackend {
    fn id(&self) -> &'static str {
        "yt-dlp"
    }
    fn capabilities(&self) -> Vec<CapabilityReport> {
        capabilities()
    }

    async fn start(&self, request: TransferRequest) -> Result<BackendHandle, BackendError> {
        validate(&request)?;
        self.config.space.ensure(&request.destination_dir, 0)?;
        let id = request.attempt_id.as_str().to_owned();
        let mut attempts = self.attempts.lock().await;
        if attempts.contains_key(&id) {
            return Err(error(ErrorKind::InvalidRequest, "attempt already started"));
        }
        let work = work_dir(&request)?;
        // create_dir, rather than create_dir_all, refuses stale output, a symlink or another
        // attempt's folder. An explicit Restart calls discard_output before trying again.
        std::fs::create_dir(&work).map_err(|_| {
            error(
                ErrorKind::DestinationUnwritable,
                "cannot create media staging folder; restart to clear this job's old partial data",
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).is_err() {
                let _ = std::fs::remove_dir(&work);
                return Err(error(
                    ErrorKind::DestinationUnwritable,
                    "cannot protect media staging folder",
                ));
            }
        }
        let progress = Arc::new(Mutex::new(BackendProgress {
            state: TransferState::Active,
            completed: ByteCount(0),
            total: None,
            connections: 1,
            naming: None,
        }));
        let (tx, rx) = mpsc::channel(1);
        let (done_tx, done) = watch::channel(false);
        let child = spawn(&self.config, &work).await;
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                let _ = std::fs::remove_dir(&work);
                return Err(e);
            }
        };
        // URL is supplied as one batch entry over a pipe, not argv, environment or a disk file.
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| error(ErrorKind::Internal, "yt-dlp input unavailable"))?;
        if stdin
            .write_all(format!("{}\n", request.url).as_bytes())
            .await
            .is_err()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = std::fs::remove_dir_all(&work);
            return Err(error(
                ErrorKind::ServiceUnavailable,
                "cannot send the media link to yt-dlp",
            ));
        }
        drop(stdin);
        tokio::spawn(run(
            child,
            rx,
            done_tx,
            Arc::clone(&progress),
            work.clone(),
            Arc::clone(&self.config.space),
        ));
        attempts.insert(
            id.clone(),
            Arc::new(Attempt {
                request,
                work,
                progress,
                control: tx,
                done,
            }),
        );
        Ok(BackendHandle(id))
    }

    async fn pause(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle).await?;
        stop(&attempt, TransferState::Paused).await
    }

    async fn resume(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle).await?;
        let snapshot = snapshot(&attempt.progress)?;
        if snapshot.state != TransferState::Paused || snapshot.completed.0 != 0 {
            return Err(error(
                ErrorKind::BackendCapabilityUnavailable,
                "yt-dlp partial data cannot be resumed safely; choose Restart",
            ));
        }
        cleanup(&attempt.work)?;
        self.attempts.lock().await.remove(&handle.0);
        self.start(attempt.request.clone()).await.map(drop)
    }

    async fn remove(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle).await?;
        stop(&attempt, TransferState::Removed).await?;
        self.attempts.lock().await.remove(&handle.0);
        // Completed output has already been placed by the service; remove the empty private
        // directory. Incomplete output is kept for an explicit Restart, never guessed resumable.
        let _ = std::fs::remove_dir(&attempt.work);
        Ok(())
    }

    async fn discard(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        let attempt = self.attempt(handle).await?;
        if snapshot(&attempt.progress)?.state == TransferState::Complete {
            return Err(error(
                ErrorKind::InvalidRequest,
                "a completed transfer cannot be discarded",
            ));
        }
        stop(&attempt, TransferState::Removed).await?;
        cleanup(&attempt.work)?;
        self.attempts.lock().await.remove(&handle.0);
        Ok(())
    }

    async fn discard_output(&self, request: &TransferRequest) -> Result<(), BackendError> {
        let work = work_dir(request)?;
        let attempts = self.attempts.lock().await;
        if attempts.values().any(|a| a.work == work) {
            return Err(error(
                ErrorKind::InvalidRequest,
                "an attempt still owns the partial data",
            ));
        }
        cleanup(&work)
    }

    async fn progress(&self, handle: &BackendHandle) -> Result<BackendProgress, BackendError> {
        snapshot(&self.attempt(handle).await?.progress)
    }

    async fn staged_output(&self, handle: &BackendHandle) -> Result<StagedOutput, BackendError> {
        let attempt = self.attempt(handle).await?;
        if snapshot(&attempt.progress)?.state != TransferState::Complete {
            return Err(error(
                ErrorKind::InvalidRequest,
                "media transfer is not complete",
            ));
        }
        Ok(StagedOutput::Staged(attempt.work.join("media")))
    }

    async fn checkpoint(
        &self,
        handle: &BackendHandle,
    ) -> Result<Option<DurableCheckpoint>, BackendError> {
        self.attempt(handle).await?;
        Ok(None)
    }

    async fn restore(
        &self,
        _: TransferRequest,
        _: &DurableCheckpoint,
    ) -> Result<BackendHandle, BackendError> {
        Err(error(
            ErrorKind::BackendCapabilityUnavailable,
            "yt-dlp has no validated durable checkpoint; choose Restart",
        ))
    }
}

fn error(kind: ErrorKind, detail: &str) -> BackendError {
    BackendError::new(kind, detail)
}

fn validate(request: &TransferRequest) -> Result<(), BackendError> {
    if !(request.url.starts_with("http://") || request.url.starts_with("https://"))
        || request.url.bytes().any(|b| b.is_ascii_control())
    {
        return Err(error(
            ErrorKind::InvalidRequest,
            "yt-dlp accepts one HTTP(S) media link",
        ));
    }
    let authority = request
        .url
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    if authority.contains('@')
        || request.session.is_some()
        || !request.headers.is_empty()
        || request.login.is_some()
    {
        return Err(error(
            ErrorKind::BackendCapabilityUnavailable,
            "scoped credentials and custom headers require the native download engine",
        ));
    }
    if request.rate_limit.is_some() {
        return Err(error(
            ErrorKind::BackendCapabilityUnavailable,
            "per-download speed limits are not integrated for yt-dlp",
        ));
    }
    work_dir(request).map(drop)
}

fn work_dir(request: &TransferRequest) -> Result<PathBuf, BackendError> {
    let mut parts = Path::new(&request.file_name).components();
    if !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || request.file_name.contains(['/', '\\'])
    {
        return Err(error(
            ErrorKind::InvalidRequest,
            "invalid media staging name",
        ));
    }
    let digest: String = Sha256::digest(request.file_name.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(request
        .destination_dir
        .join(format!(".downova-ytdlp-{digest}")))
}

fn cleanup(work: &Path) -> Result<(), BackendError> {
    match std::fs::symlink_metadata(work) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(meta) if meta.is_dir() && !meta.is_symlink() => {
            std::fs::remove_dir_all(work).map_err(|_| {
                error(
                    ErrorKind::DestinationUnwritable,
                    "cannot clear media partial data",
                )
            })
        }
        _ => Err(error(
            ErrorKind::DestinationUnwritable,
            "refusing unexpected media staging entry",
        )),
    }
}

async fn spawn(config: &LaunchConfig, work: &Path) -> Result<tokio::process::Child, BackendError> {
    let mut cmd = tokio::process::Command::new(&config.binary.path);
    if let Some(ca) = &config.ca_file {
        cmd.env("SSL_CERT_FILE", ca);
    }
    cmd.args([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-update",
        "--no-playlist",
        "--playlist-items",
        "1",
        "--compat-options",
        "no-certifi",
        "--no-continue",
        "--no-overwrites",
        "--no-cache-dir",
        "--no-colors",
        "--no-warnings",
        "--newline",
        "--progress",
        "--no-simulate",
        "--socket-timeout",
        "20",
        "--retries",
        "0",
        "--fragment-retries",
        "0",
        "--abort-on-unavailable-fragments",
        "--concurrent-fragments",
        "1",
        "--downloader",
        "native",
        "--hls-prefer-native",
        "--fixup",
        "never",
        "--format",
        "best[protocol=https]/best[protocol=http]/best[protocol=m3u8_native]/best[protocol=http_dash_segments]",
        "--match-filter",
        "!is_live & !is_upcoming",
        "--js-runtimes",
        "node",
        "--proxy",
        "",
        "--output",
        "media",
        "--batch-file",
        "-",
        "--progress-template",
        "download:downova-progress:%(progress)j",
        "--print",
        "before_dl:downova-meta:%(.{title,ext})j",
    ])
    .current_dir(work)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .map_err(|_| error(ErrorKind::ServiceUnavailable, "cannot start yt-dlp"))
}

fn snapshot(progress: &Mutex<BackendProgress>) -> Result<BackendProgress, BackendError> {
    progress
        .lock()
        .map(|p| p.clone())
        .map_err(|_| error(ErrorKind::Internal, "media state unavailable"))
}

fn set_state(progress: &Mutex<BackendProgress>, state: TransferState) {
    if let Ok(mut p) = progress.lock() {
        p.state = state;
        p.connections = 0;
    }
}

async fn stop(attempt: &Attempt, state: TransferState) -> Result<(), BackendError> {
    if *attempt.done.borrow() {
        return Ok(());
    }
    let (ack, done) = oneshot::channel();
    if attempt.control.send(Stop { state, ack }).await.is_ok() {
        tokio::time::timeout(Duration::from_secs(10), done)
            .await
            .map_err(|_| error(ErrorKind::ServiceUnavailable, "yt-dlp did not stop in time"))?
            .or_else(|_| {
                if *attempt.done.borrow() {
                    Ok(())
                } else {
                    Err(error(
                        ErrorKind::ServiceUnavailable,
                        "yt-dlp stop acknowledgement lost",
                    ))
                }
            })?;
    } else {
        let mut rx = attempt.done.clone();
        tokio::time::timeout(Duration::from_secs(10), rx.wait_for(|done| *done))
            .await
            .map_err(|_| error(ErrorKind::ServiceUnavailable, "yt-dlp did not exit in time"))?
            .map_err(|_| error(ErrorKind::ServiceUnavailable, "yt-dlp exit unavailable"))?;
    }
    Ok(())
}

async fn run(
    mut child: tokio::process::Child,
    mut controls: mpsc::Receiver<Stop>,
    done: watch::Sender<bool>,
    progress: Arc<Mutex<BackendProgress>>,
    work: PathBuf,
    space: Arc<dyn SpaceGuard>,
) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let p = Arc::clone(&progress);
    let dir = work.clone();
    let reader = tokio::spawn(async move {
        let Some(stdout) = stdout else {
            return;
        };
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        loop {
            // Consume through a bounded `take`, preventing an unbounded line allocation. A long
            // line is dropped, never logged; the next read continues draining the pipe.
            bytes.clear();
            let n = match (&mut reader)
                .take(64 * 1024)
                .read_until(b'\n', &mut bytes)
                .await
            {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            if n == 64 * 1024 {
                continue;
            }
            if let Ok(line) = std::str::from_utf8(&bytes) {
                parse_line(line.trim(), &p);
            }
            if let Ok(current) = snapshot(&p) {
                let remaining = current
                    .total
                    .map_or(0, |n| n.0.saturating_sub(current.completed.0));
                if let Err(e) = space.ensure(&dir, remaining) {
                    set_state(&p, TransferState::Failed(e));
                    break;
                }
            }
        }
    });
    let errors = tokio::spawn(async move {
        let mut tail = Vec::new();
        if let Some(mut stderr) = stderr {
            let mut buf = [0; 2048];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                // Keep only a bounded tail for classification. Raw text never leaves this task.
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > 16 * 1024 {
                    tail.drain(..tail.len().saturating_sub(16 * 1024));
                }
            }
        }
        classify(&String::from_utf8_lossy(&tail))
    });
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut stopped = None;
    let status = loop {
        tokio::select! {
            status = child.wait() => break status,
            request = controls.recv() => {
                stopped = request;
                let _ = child.start_kill();
                break child.wait().await;
            }
            _ = tick.tick() => {
                if snapshot(&progress).is_ok_and(|p| matches!(p.state, TransferState::Failed(_))) {
                    let _ = child.start_kill();
                    break child.wait().await;
                }
            }
        }
    };
    let _ = reader.await;
    let failure = errors
        .await
        .unwrap_or_else(|_| error(ErrorKind::ServiceUnavailable, "yt-dlp exited unexpectedly"));
    if let Some(request) = stopped {
        set_state(&progress, request.state);
        let _ = done.send(true);
        let _ = request.ack.send(());
        return;
    }
    if !snapshot(&progress).is_ok_and(|p| matches!(p.state, TransferState::Failed(_))) {
        match status {
            Ok(status) if status.success() => {
                let result = complete(&work.join("media"), &progress);
                if let Err(e) = result {
                    set_state(&progress, TransferState::Failed(e));
                }
            }
            _ => set_state(&progress, TransferState::Failed(failure)),
        }
    }
    let _ = done.send(true);
}

fn complete(output: &Path, progress: &Mutex<BackendProgress>) -> Result<(), BackendError> {
    let meta = std::fs::symlink_metadata(output).map_err(|_| {
        error(
            ErrorKind::ServiceUnavailable,
            "yt-dlp exited without producing a media file",
        )
    })?;
    if !meta.is_file() || meta.is_symlink() || meta.len() == 0 {
        return Err(error(
            ErrorKind::ServiceUnavailable,
            "yt-dlp did not produce a regular nonempty media file",
        ));
    }
    // Windows flushes (FlushFileBuffers) only through a handle with write access; a read-only
    // handle fails with "access denied". The file is opened for writing but never changed.
    std::fs::OpenOptions::new()
        .write(true)
        .open(output)
        .and_then(|file| file.sync_all())
        .map_err(|_| {
            error(
                ErrorKind::DestinationUnwritable,
                "cannot sync the downloaded media",
            )
        })?;
    let mut p = progress
        .lock()
        .map_err(|_| error(ErrorKind::Internal, "media state unavailable"))?;
    p.completed = ByteCount(meta.len());
    p.total = Some(p.completed);
    p.connections = 0;
    p.state = TransferState::Complete;
    Ok(())
}

fn parse_line(line: &str, progress: &Mutex<BackendProgress>) {
    let Ok(mut p) = progress.lock() else {
        return;
    };
    if let Some(raw) = line.strip_prefix("downova-progress:") {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            if let Some(n) = v["downloaded_bytes"].as_u64() {
                p.completed = ByteCount(n);
            }
            p.total = v["total_bytes"].as_u64().map(ByteCount);
        }
        return;
    }
    let Some(raw) = line.strip_prefix("downova-meta:") else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return;
    };
    let (Some(title), Some(ext)) = (v["title"].as_str(), v["ext"].as_str()) else {
        return;
    };
    if title.is_empty()
        || title.len() > 2048
        || ext.is_empty()
        || ext.len() > 12
        || !ext.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return;
    }
    let name = format!("{title}.{ext}");
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    p.naming = Some(ResponseNaming {
        content_disposition: None,
        url_file_name: Some(encoded),
    });
}

fn classify(raw: &str) -> BackendError {
    let text = raw.to_ascii_lowercase();
    let (kind, detail) = if text.contains("no space left") || text.contains("disk full") {
        (ErrorKind::DiskFull, "not enough disk space for the media")
    } else if text.contains("certificate_verify_failed")
        || text.contains("certificate verify failed")
    {
        (
            ErrorKind::TlsHandshakeFailed,
            "the media server's certificate could not be verified",
        )
    } else if text.contains("http error 429") || text.contains("http error 5") {
        (
            ErrorKind::ServerBusy,
            "the media server is busy or is limiting requests",
        )
    } else if text.contains("http error 401")
        || text.contains("http error 403")
        || text.contains("sign in")
    {
        (
            ErrorKind::AuthRequired,
            "the media requires sign-in; scoped sign-in is not integrated for yt-dlp",
        )
    } else if text.contains("unsupported url")
        || text.contains("requested format is not available")
        || text.contains("match filter")
        || text.contains("no video formats found")
    {
        (
            ErrorKind::BackendCapabilityUnavailable,
            "this link has no supported single-item media format",
        )
    } else if text.contains("http error 404") {
        (
            ErrorKind::LinkRefreshRequired,
            "the media link is no longer available",
        )
    } else {
        (
            ErrorKind::ServiceUnavailable,
            "yt-dlp could not download this media; check the link and the installed yt-dlp version",
        )
    };
    error(kind, detail)
}

#[cfg(test)]
mod tests;
