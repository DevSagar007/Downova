//! The transfer task: TLS client, manual redirects, response validation, streaming to disk and
//! publishing the finished file.

use std::path::Path;
use std::sync::Arc;

use dm_backend_api::{BackendError, TransferState, require_capability};
use dm_domain::redact::is_sensitive_header;
use dm_domain::{Capability, CapabilityReport, ErrorKind};
use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, IF_RANGE,
    LOCATION, RANGE, RETRY_AFTER,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use crate::classify::{io_error, retry_after_header, status_error, transport_error};
use crate::{Attempt, HttpConfig, lock};

/// Write buffer between the network and the partial file (bounded memory per attempt).
const WRITE_BUFFER: usize = 256 * 1024;

/// Request headers that are bound to the origin the caller supplied them for: credentials and
/// conditional-request validators. They are dropped on any cross-origin redirect hop.
fn origin_scoped(name: &str) -> bool {
    is_sensitive_header(name)
        || [
            "if-match",
            "if-none-match",
            "if-range",
            "if-modified-since",
            "if-unmodified-since",
        ]
        .iter()
        .any(|v| name.eq_ignore_ascii_case(v))
}

pub(crate) fn build_client(config: &HttpConfig) -> Result<reqwest::Client, BackendError> {
    let fail = |what: &str| BackendError::new(ErrorKind::Internal, format!("TLS setup: {what}"));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = if config.extra_roots.is_empty() {
        rustls_platform_verifier::Verifier::new(Arc::clone(&provider))
    } else {
        rustls_platform_verifier::Verifier::new_with_extra_roots(
            config.extra_roots.clone(),
            Arc::clone(&provider),
        )
    }
    .map_err(|_| fail("platform verifier unavailable"))?;
    // `dangerous()` is only rustls' entry point for a custom verifier: the platform verifier
    // performs full chain, name and (where the OS does) revocation checks.
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| fail("protocol versions"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .referer(false)
        .connect_timeout(config.connect_timeout)
        .read_timeout(config.read_timeout)
        .user_agent(concat!("Downova/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| fail("client"))
}

/// Gates a URL's scheme on the capability report (first URL and every redirect hop).
pub(crate) fn gate_url(url: &str, report: &[CapabilityReport]) -> Result<Url, BackendError> {
    let url =
        Url::parse(url).map_err(|_| BackendError::new(ErrorKind::InvalidRequest, "invalid URL"))?;
    gate(&url, report)?;
    Ok(url)
}

fn gate(url: &Url, report: &[CapabilityReport]) -> Result<(), BackendError> {
    let capability = match url.scheme() {
        "http" => Capability::Http,
        "https" => Capability::Https,
        _ => {
            return Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "redirect or URL to an unsupported scheme",
            ));
        }
    };
    require_capability(report, capability)
}

/// Creates the attempt's partial file exclusively; refuses if the destination already exists.
pub(crate) fn create_part(destination: &Path, part: &Path) -> Result<(), BackendError> {
    if destination.exists() {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "target file already exists",
        ));
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(part)
        .map(drop)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => BackendError::new(
                ErrorKind::InvalidRequest,
                "a partial file with this name already exists",
            ),
            _ => io_error("cannot create partial file", &e),
        })
}

/// Empties the attempt's own partial file before a restart from zero.
pub(crate) fn truncate_part(part: &Path) -> Result<(), BackendError> {
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(part)
        .map(drop)
        .map_err(|e| io_error("cannot reset partial file", &e))
}

/// Why a transfer ended without completing.
enum Ended {
    Stopped,
    Failed(BackendError),
}

impl From<BackendError> for Ended {
    fn from(e: BackendError) -> Self {
        Ended::Failed(e)
    }
}

/// The representation a transfer is bound to, fixed by the first `200`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    /// Strong entity tag (`"…"`); `None` when the server sent none or only a weak one.
    pub(crate) etag: Option<String>,
    /// Representation length from `Content-Length` of the `200`.
    pub(crate) length: Option<u64>,
    /// The final URL after redirects: the resource the ETag belongs to.
    pub(crate) url: Url,
}

/// Confirmed on-disk state: `offset` bytes flushed to the partial file, and the SHA-256 state of
/// exactly those bytes (used to detect a partial file changed outside the backend).
#[derive(Clone)]
pub(crate) struct Checkpoint {
    pub(crate) identity: Identity,
    pub(crate) offset: u64,
    pub(crate) hasher: Sha256,
}

impl std::fmt::Debug for Checkpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkpoint")
            .field("etag", &self.identity.etag)
            .field("length", &self.identity.length)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl Checkpoint {
    /// Resume needs a strong validator and a known length; otherwise it is explicitly unavailable.
    pub(crate) fn check_resumable(&self) -> Result<(), BackendError> {
        let missing = match (&self.identity.etag, self.identity.length) {
            (Some(_), Some(_)) => return Ok(()),
            (None, _) => "the server gave no strong validator (ETag)",
            (Some(_), None) => "the server did not announce the file length",
        };
        Err(BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            format!(
                "cannot resume safely: {missing}; the download can only restart from the beginning"
            ),
        ))
    }
}

/// Re-reads the partial file and compares length and SHA-256 with the checkpoint.
pub(crate) async fn verify_partial(part: &Path, cp: &Checkpoint) -> Result<(), BackendError> {
    let part = part.to_owned();
    let (offset, expected) = (cp.offset, cp.hasher.clone().finalize());
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
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
        if len != offset {
            return Err(changed(if len < offset {
                "truncated"
            } else {
                "extended"
            }));
        }
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; WRITE_BUFFER];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| io_error("cannot read partial file", &e))?;
            if n == 0 {
                break;
            }
            hasher.update(buf.get(..n).unwrap_or_default());
        }
        if hasher.finalize() != expected {
            return Err(changed("modified"));
        }
        Ok(())
    })
    .await
    .map_err(|_| BackendError::new(ErrorKind::Internal, "partial file check failed"))?
}

pub(crate) async fn run(
    client: reqwest::Client,
    config: HttpConfig,
    attempt: Arc<Attempt>,
    mut stop: watch::Receiver<bool>,
) {
    let outcome = match fetch(&client, &config, &attempt, &mut stop).await {
        Ok(()) => publish(&attempt.part, &attempt.destination).map_err(Ended::Failed),
        Err(e) => Err(e),
    };
    let mut shared = lock(&attempt.shared);
    match outcome {
        Ok(()) => shared.state = TransferState::Complete,
        Err(Ended::Failed(e)) => shared.state = TransferState::Failed(e),
        // The controller that stopped the task sets the resulting state.
        Err(Ended::Stopped) => {}
    }
}

/// Resolves `fut` unless the stop switch is flipped first.
async fn or_stop<T>(
    stop: &mut watch::Receiver<bool>,
    fut: impl std::future::Future<Output = T>,
) -> Result<T, Ended> {
    if *stop.borrow() {
        return Err(Ended::Stopped);
    }
    tokio::select! {
        biased;
        _ = stop.changed() => Err(Ended::Stopped),
        value = fut => Ok(value),
    }
}

/// A strong entity tag from the response, if any. Weak (`W/…`) and malformed tags yield `None`.
fn strong_etag(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(ETAG)?.to_str().ok()?.trim();
    let quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
    quoted.then(|| value.to_owned())
}

fn ensure_identity_encoding(headers: &HeaderMap) -> Result<(), BackendError> {
    match headers.get(CONTENT_ENCODING) {
        Some(encoding)
            if !encoding
                .to_str()
                .is_ok_and(|e| e.trim().eq_ignore_ascii_case("identity")) =>
        {
            Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "the server sent an encoded body although identity was requested",
            ))
        }
        _ => Ok(()),
    }
}

fn status_failure(response: &reqwest::Response) -> BackendError {
    let wait = retry_after_header(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
    );
    status_error(response.status().as_u16(), wait)
}

/// Headers the caller supplied, scoped to the request's original origin.
fn with_caller_headers(
    mut req: reqwest::RequestBuilder,
    attempt: &Attempt,
    same_origin: bool,
) -> reqwest::RequestBuilder {
    for (name, value) in &attempt.request.headers {
        if same_origin || !origin_scoped(name) {
            req = req.header(name.as_str(), value.expose());
        }
    }
    req
}

/// The first request: follows redirects manually and returns the final `200` and its URL.
async fn initial_request(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(reqwest::Response, Url), Ended> {
    let first = gate_url(&attempt.request.url, &config.scheme_gate)?;
    let origin = first.origin();
    let mut url = first;
    let mut hops: u8 = 0;
    loop {
        let req = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
        let req = with_caller_headers(req, attempt, url.origin() == origin);
        let response = or_stop(stop, req.send())
            .await?
            .map_err(|e| transport_error(&e))?;
        let status = response.status();
        if status.is_redirection() && status != reqwest::StatusCode::NOT_MODIFIED {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    BackendError::new(ErrorKind::InvalidRequest, "redirect without a location")
                })?;
            let next = url.join(location).map_err(|_| {
                BackendError::new(ErrorKind::InvalidRequest, "redirect to an invalid URL")
            })?;
            hops = hops.saturating_add(1);
            if hops > config.max_redirects {
                return Err(
                    BackendError::new(ErrorKind::InvalidRequest, "too many redirects").into(),
                );
            }
            gate(&next, &config.scheme_gate)?;
            url = next;
            continue;
        }
        if status != reqwest::StatusCode::OK {
            return Err(status_failure(&response).into());
        }
        return Ok((response, url));
    }
}

/// Outcome of a resume request.
enum Resumed {
    /// A validated `206` continuing at the checkpoint offset.
    Continue(reqwest::Response),
    /// A `200`: the full (possibly new) representation; the file restarts from zero.
    Restart(reqwest::Response, Identity),
}

/// `Range` + `If-Range` to the resource the checkpoint belongs to; no redirects are followed.
async fn resume_request(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    cp: &Checkpoint,
    stop: &mut watch::Receiver<bool>,
) -> Result<Resumed, Ended> {
    let url = cp.identity.url.clone();
    gate(&url, &config.scheme_gate)?;
    let etag = cp.identity.etag.clone().ok_or_else(|| {
        BackendError::new(
            ErrorKind::BackendCapabilityUnavailable,
            "no strong validator",
        )
    })?;
    let origin = gate_url(&attempt.request.url, &config.scheme_gate)?.origin();
    let req = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={}-", cp.offset))
        .header(IF_RANGE, etag);
    let req = with_caller_headers(req, attempt, url.origin() == origin);
    let response = or_stop(stop, req.send())
        .await?
        .map_err(|e| transport_error(&e))?;
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {
            validate_partial_response(response.headers(), cp)?;
            Ok(Resumed::Continue(response))
        }
        reqwest::StatusCode::OK => {
            ensure_identity_encoding(response.headers())?;
            let identity = Identity {
                etag: strong_etag(response.headers()),
                length: response.content_length(),
                url,
            };
            Ok(Resumed::Restart(response, identity))
        }
        s if s.is_redirection() => Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file moved since the download started; it must restart",
        )
        .into()),
        _ => Err(status_failure(&response).into()),
    }
}

/// Every check that must pass before a single byte of a `206` is appended.
pub(crate) fn validate_partial_response(
    headers: &HeaderMap,
    cp: &Checkpoint,
) -> Result<(), BackendError> {
    let length = cp
        .identity
        .length
        .ok_or_else(|| BackendError::new(ErrorKind::BackendCapabilityUnavailable, "no length"))?;
    ensure_identity_encoding(headers)?;
    if strong_etag(headers) != cp.identity.etag {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file on the server changed (ETag differs); nothing was appended",
        ));
    }
    let range = headers
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range)
        .ok_or_else(|| {
            BackendError::new(
                ErrorKind::InvalidRange,
                "missing or malformed Content-Range; nothing was appended",
            )
        })?;
    if range.total != length {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file length on the server changed; nothing was appended",
        ));
    }
    let expected_last = length.checked_sub(1);
    if range.first != cp.offset || Some(range.last) != expected_last {
        return Err(BackendError::new(
            ErrorKind::InvalidRange,
            "the server returned a different byte range than requested; nothing was appended",
        ));
    }
    let span = range
        .last
        .checked_sub(range.first)
        .and_then(|d| d.checked_add(1));
    if let Some(declared) = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        && Some(declared) != span
    {
        return Err(BackendError::new(
            ErrorKind::InvalidRange,
            "Content-Length does not match Content-Range; nothing was appended",
        ));
    }
    Ok(())
}

/// `bytes first-last/total` with `first <= last < total`.
struct ContentRange {
    first: u64,
    last: u64,
    total: u64,
}

fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (span, total) = rest.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    let digits = |s: &str| {
        let s = s.trim();
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let (first, last, total) = (digits(first)?, digits(last)?, digits(total)?);
    (first <= last && last < total).then_some(ContentRange { first, last, total })
}

async fn fetch(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let plan = lock(&attempt.resume_from).take();
    let (response, identity, start, mut hasher) = match plan {
        Some(cp) => match resume_request(client, config, attempt, &cp, stop).await? {
            Resumed::Continue(response) => (response, cp.identity, cp.offset, cp.hasher),
            Resumed::Restart(response, identity) => (response, identity, 0, Sha256::new()),
        },
        None => {
            let (response, url) = initial_request(client, config, attempt, stop).await?;
            ensure_identity_encoding(response.headers())?;
            let identity = Identity {
                etag: strong_etag(response.headers()),
                length: response.content_length(),
                url,
            };
            (response, identity, 0, Sha256::new())
        }
    };

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(start == 0)
        .open(&attempt.part)
        .await
        .map_err(|e| io_error("cannot open partial file", &e))?;
    if start > 0 {
        // The resume point was verified; re-check right before appending.
        let on_disk = file
            .metadata()
            .await
            .map_err(|e| io_error("cannot read partial file", &e))?
            .len();
        if on_disk != start {
            return Err(BackendError::new(
                ErrorKind::InvalidRange,
                "the partial file changed before appending; nothing was appended",
            )
            .into());
        }
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| io_error("cannot position partial file", &e))?;
    }
    {
        let mut shared = lock(&attempt.shared);
        shared.state = TransferState::Active;
        shared.total = identity.length;
        shared.completed = start;
    }
    *lock(&attempt.checkpoint) = Some(Checkpoint {
        identity: identity.clone(),
        offset: start,
        hasher: hasher.clone(),
    });

    let mut out = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);
    let mut body = response.bytes_stream();
    let mut written: u64 = start;
    loop {
        let next_chunk = match or_stop(stop, body.next()).await {
            Ok(next_chunk) => next_chunk,
            Err(ended) => {
                // Stopping: flush, then record what is confirmed on disk, so the partial file
                // matches the reported progress and the resume point.
                if out.flush().await.is_ok() {
                    *lock(&attempt.checkpoint) = Some(Checkpoint {
                        identity,
                        offset: written,
                        hasher,
                    });
                }
                return Err(ended);
            }
        };
        let chunk = match next_chunk {
            None => break,
            Some(Err(e)) => {
                // Bytes already accepted passed every validation: keep them on disk so the
                // partial file matches the reported progress.
                let _ = out.flush().await;
                return Err(transport_error(&e).into());
            }
            Some(Ok(chunk)) => chunk,
        };
        let next = u64::try_from(chunk.len())
            .ok()
            .and_then(|n| written.checked_add(n))
            .ok_or_else(|| BackendError::new(ErrorKind::Internal, "byte count overflow"))?;
        if identity.length.is_some_and(|total| next > total) {
            return Err(BackendError::new(
                ErrorKind::InvalidRange,
                "the server sent more bytes than it announced",
            )
            .into());
        }
        out.write_all(&chunk)
            .await
            .map_err(|e| io_error("cannot write partial file", &e))?;
        hasher.update(&chunk);
        written = next;
        lock(&attempt.shared).completed = written;
    }
    out.flush()
        .await
        .map_err(|e| io_error("cannot write partial file", &e))?;
    if let Some(total) = identity.length
        && written != total
    {
        return Err(BackendError::new(
            ErrorKind::NetworkOffline,
            format!("connection closed after {written} of {total} bytes"),
        )
        .into());
    }
    out.get_ref()
        .sync_all()
        .await
        .map_err(|e| io_error("cannot flush partial file", &e))?;
    Ok(())
}

/// Moves the finished partial file to its destination without ever replacing an existing file:
/// a hard link fails if the destination exists. Where hard links are unavailable, falls back to
/// a rename after an existence check.
fn publish(part: &Path, destination: &Path) -> Result<(), BackendError> {
    let collision = || {
        BackendError::new(
            ErrorKind::InvalidRequest,
            "target file appeared during the transfer; it was not overwritten",
        )
    };
    match std::fs::hard_link(part, destination) {
        Ok(()) => std::fs::remove_file(part)
            .map_err(|e| io_error("cannot remove partial file after publishing", &e)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(collision()),
        Err(_) => {
            if destination.exists() {
                return Err(collision());
            }
            std::fs::rename(part, destination)
                .map_err(|e| io_error("cannot publish finished file", &e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_domain::CapabilitySupport;

    fn report(http: CapabilitySupport, https: CapabilitySupport) -> Vec<CapabilityReport> {
        [(Capability::Http, http), (Capability::Https, https)]
            .into_iter()
            .map(|(capability, support)| CapabilityReport {
                capability,
                support,
                note: None,
            })
            .collect()
    }

    #[test]
    fn every_hop_is_gated_by_the_report() {
        use CapabilitySupport::{Supported, Unverified};
        let r = report(Supported, Unverified);
        assert!(gate_url("http://h/f", &r).is_ok());
        let e = gate_url("https://h/f", &r).unwrap_err();
        assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable);
        let all = report(Supported, Supported);
        for other in ["ftp://h/f", "file:///etc/passwd", "data:,x"] {
            assert_eq!(
                gate_url(other, &all).unwrap_err().kind,
                ErrorKind::BackendCapabilityUnavailable,
                "{other}"
            );
        }
    }

    #[test]
    fn credentials_and_validators_are_origin_scoped() {
        for name in [
            "Authorization",
            "cookie",
            "Proxy-Authorization",
            "If-Match",
            "if-range",
        ] {
            assert!(origin_scoped(name), "{name}");
        }
        for name in ["Accept", "User-Agent", "X-Requested-With"] {
            assert!(!origin_scoped(name), "{name}");
        }
    }

    fn checkpoint(offset: u64, length: u64) -> Checkpoint {
        Checkpoint {
            identity: Identity {
                etag: Some("\"v1\"".into()),
                length: Some(length),
                url: Url::parse("http://h/f").unwrap(),
            },
            offset,
            hasher: Sha256::new(),
        }
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (n, v) in pairs {
            h.insert(*n, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn content_range_parsing_is_strict() {
        let ok = parse_content_range("bytes 100-199/200").unwrap();
        assert_eq!((ok.first, ok.last, ok.total), (100, 199, 200));
        for bad in [
            "bytes 100-99/200",
            "bytes 100-200/200",
            "bytes */200",
            "bytes 100-199/*",
            "bytes -5-199/200",
            "items 0-1/2",
            "bytes 1 00-199/200",
            "bytes 0-18446744073709551616/18446744073709551617",
        ] {
            assert!(parse_content_range(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_206_is_accepted_only_when_every_check_passes() {
        let cp = checkpoint(100, 200);
        let good = [
            ("etag", "\"v1\""),
            ("content-range", "bytes 100-199/200"),
            ("content-length", "100"),
        ];
        assert!(validate_partial_response(&headers(&good), &cp).is_ok());
        let kind = |h: &[(&'static str, &str)]| {
            validate_partial_response(&headers(h), &cp)
                .unwrap_err()
                .kind
        };
        assert_eq!(
            kind(&[("etag", "\"v2\""), ("content-range", "bytes 100-199/200")]),
            ErrorKind::RemoteChanged
        );
        assert_eq!(
            kind(&[("etag", "W/\"v1\""), ("content-range", "bytes 100-199/200")]),
            ErrorKind::RemoteChanged
        );
        assert_eq!(
            kind(&[("content-range", "bytes 100-199/200")]),
            ErrorKind::RemoteChanged
        );
        assert_eq!(
            kind(&[("etag", "\"v1\""), ("content-range", "bytes 101-199/200")]),
            ErrorKind::InvalidRange
        );
        assert_eq!(
            kind(&[("etag", "\"v1\""), ("content-range", "bytes 100-150/200")]),
            ErrorKind::InvalidRange
        );
        assert_eq!(
            kind(&[("etag", "\"v1\""), ("content-range", "bytes 100-299/300")]),
            ErrorKind::RemoteChanged
        );
        assert_eq!(kind(&[("etag", "\"v1\"")]), ErrorKind::InvalidRange);
        assert_eq!(
            kind(&[
                ("etag", "\"v1\""),
                ("content-range", "bytes 100-199/200"),
                ("content-length", "99")
            ]),
            ErrorKind::InvalidRange
        );
        assert_eq!(
            kind(&[
                ("etag", "\"v1\""),
                ("content-range", "bytes 100-199/200"),
                ("content-encoding", "gzip")
            ]),
            ErrorKind::InvalidRequest
        );
    }

    #[test]
    fn resume_needs_a_strong_etag_and_a_length() {
        let mut cp = checkpoint(10, 20);
        assert!(cp.check_resumable().is_ok());
        cp.identity.length = None;
        assert_eq!(
            cp.check_resumable().unwrap_err().kind,
            ErrorKind::BackendCapabilityUnavailable
        );
        cp.identity.length = Some(20);
        cp.identity.etag = None;
        assert_eq!(
            cp.check_resumable().unwrap_err().kind,
            ErrorKind::BackendCapabilityUnavailable
        );
    }

    #[test]
    fn publish_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("dm-http-publish-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("a.bin.downova-part");
        let dest = dir.join("a.bin");
        std::fs::write(&part, b"new").unwrap();
        std::fs::write(&dest, b"user file").unwrap();
        assert_eq!(
            publish(&part, &dest).unwrap_err().kind,
            ErrorKind::InvalidRequest
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"user file");
        std::fs::remove_file(&dest).unwrap();
        publish(&part, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!part.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
