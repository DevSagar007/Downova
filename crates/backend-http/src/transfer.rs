//! The transfer task: TLS client, manual redirects, response validation and streaming to disk.
//! A finished transfer stays in its synced staging (partial) file: verification and placement at
//! the destination belong to the service's finalization journal (B1.3, ADR 0008).

use std::path::Path;
use std::sync::Arc;

use dm_backend_api::{
    BackendError, MAX_DISPOSITION_BYTES, ResponseNaming, TransferState, require_capability,
};
use dm_domain::redact::is_sensitive_header;
use dm_domain::{Capability, CapabilityReport, ErrorKind};
use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, DATE, ETAG, HeaderMap, IF_RANGE, LAST_MODIFIED, LOCATION, RANGE, RETRY_AFTER,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use crate::classify::{io_error, retry_after_header, status_error, transport_error};
use crate::limits::Permit;
use crate::{Attempt, HttpConfig, lock};
use crate::{fault, segmented};

/// Write buffer between the network and the partial file (bounded memory per attempt).
pub(crate) const WRITE_BUFFER: usize = 256 * 1024;

/// A running writer flushes, `sync_data`s and then publishes its confirmed state (the durable
/// checkpoint the service can export) at least every [`SYNC_EVERY`] or [`SYNC_BYTES`], whichever
/// comes first (B1.2, ADR 0007). Unsynced bytes are never part of a published checkpoint.
pub(crate) const SYNC_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
pub(crate) const SYNC_BYTES: u64 = 4 * 1024 * 1024;

/// Request headers that are bound to the origin the caller supplied them for: credentials and
/// conditional-request validators. They are dropped on any cross-origin redirect hop.
pub(crate) fn origin_scoped(name: &str) -> bool {
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

/// The reqwest proxy for `proxy` (F06, ADR 0034): every request (http, and https through
/// CONNECT) goes through it, never directly. The URL carries no credentials; Basic sign-in is set
/// separately and never formatted.
fn proxy_for(proxy: &dm_backend_api::ProxyConfig) -> Result<reqwest::Proxy, BackendError> {
    let refused = || BackendError::new(ErrorKind::InvalidRequest, "the proxy address is not valid");
    let host = if proxy.host.contains(':') && !proxy.host.starts_with('[') {
        format!("[{}]", proxy.host)
    } else {
        proxy.host.clone()
    };
    let url = Url::parse(&format!("http://{host}:{}/", proxy.port)).map_err(|_| refused())?;
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(refused());
    }
    let mut p = reqwest::Proxy::all(url.as_str()).map_err(|_| refused())?;
    if let Some(user) = &proxy.username {
        let password = proxy.password.as_ref().map_or("", |s| s.expose());
        p = p.basic_auth(user, password);
    }
    Ok(p)
}

pub(crate) fn build_client(
    config: &HttpConfig,
    proxy: Option<&dm_backend_api::ProxyConfig>,
) -> Result<reqwest::Client, BackendError> {
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
    // No system proxy ever (`no_proxy` clears the environment's); the configured one, if any,
    // is the only route: a request that cannot go through it fails instead of going direct.
    let builder = match proxy {
        Some(p) => reqwest::Client::builder().no_proxy().proxy(proxy_for(p)?),
        None => reqwest::Client::builder().no_proxy(),
    };
    builder
        .tls_backend_preconfigured(tls)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        // No idle pooled connections: a request's connection closes with its permit, so open
        // connections per host stay within the per-host budget (B1.5, limits.rs).
        .pool_max_idle_per_host(0)
        .retry(reqwest::retry::never())
        .referer(false)
        .connect_timeout(config.connect_timeout)
        // No client-level read timeout (ADR 0024 review): reqwest keeps that deadline running
        // while the body is not polled, so a bandwidth wait would count as network silence and
        // fail as NetworkOffline. The engine bounds each network wait itself (`within`).
        .user_agent(format!("Downova/{}", dm_domain::BUILD_VERSION))
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

pub(crate) fn gate(url: &Url, report: &[CapabilityReport]) -> Result<(), BackendError> {
    let capability = match url.scheme() {
        "http" => Capability::Http,
        "https" => Capability::Https,
        // F05 (ADR 0035): a first URL only; an HTTP redirect to ftp:// is refused below.
        "ftp" => Capability::Ftp,
        // F05 (ADR 0037): a first URL only; an HTTP redirect to sftp:// is refused below.
        "sftp" => Capability::Sftp,
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

/// The disk budget check configured for the backend that owns `attempt`.
pub(crate) fn config_space(attempt: &Attempt) -> std::sync::Arc<dyn dm_backend_api::SpaceGuard> {
    std::sync::Arc::clone(&attempt.space)
}

/// Why a transfer ended without completing.
pub(crate) enum Ended {
    Stopped,
    Failed(BackendError),
    /// A segment saw a `200`: restart the whole file on one connection.
    Restart,
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
    /// R01 (ADR 0044): a strong `Last-Modified` (at least one second before the response's
    /// `Date`), the resume validator when the server sends no strong ETag. Exactly as sent.
    pub(crate) last_modified: Option<String>,
}

impl Identity {
    /// The identity a full `200` establishes.
    pub(crate) fn of_response(headers: &HeaderMap, length: Option<u64>, url: Url) -> Self {
        Self {
            etag: strong_etag(headers),
            length,
            url,
            last_modified: strong_last_modified(headers),
        }
    }
}

/// Bytes re-fetched before a `Last-Modified`-validated resume point and compared with the
/// partial file (ADR 0044): a date alone does not prove the bytes are the same.
pub(crate) const OVERLAP_BYTES: u64 = 64 * 1024;

/// `Last-Modified` usable as a strong validator (RFC 9110 §8.8.2.2): a valid HTTP date at least
/// one second before the response's own `Date`. Anything else is `None`.
pub(crate) fn strong_last_modified(headers: &HeaderMap) -> Option<String> {
    let lm = headers.get(LAST_MODIFIED)?.to_str().ok()?.trim();
    let date = headers.get(DATE)?.to_str().ok()?;
    let (lm_at, date_at) = (
        dm_domain::retry::parse_http_date(lm)?,
        dm_domain::retry::parse_http_date(date)?,
    );
    (date_at > lm_at).then(|| lm.to_owned())
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
        let validator = self.identity.etag.is_some() || self.identity.last_modified.is_some();
        let missing = match (validator, self.identity.length) {
            (true, Some(_)) => return Ok(()),
            (false, _) => "the server gave no strong validator (ETag or Last-Modified)",
            (true, None) => "the server did not announce the file length",
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
    let outcome = fetch(&client, &config, &attempt, &mut stop).await;
    let mut shared = lock(&attempt.shared);
    match outcome {
        Ok(()) => shared.state = TransferState::Complete,
        Err(Ended::Failed(e)) => shared.state = TransferState::Failed(e),
        // The controller that stopped the task sets the resulting state.
        Err(Ended::Stopped) => {}
        Err(Ended::Restart) => {
            shared.state = TransferState::Failed(BackendError::new(
                ErrorKind::Internal,
                "unhandled restart request",
            ));
        }
    }
}

/// Bounds one wait for the network (response headers or one body chunk) by `limit`; a timeout is
/// the same `NetworkOffline` reqwest's read timeout reported. Bandwidth waits happen outside it.
pub(crate) async fn within<T>(
    limit: std::time::Duration,
    fut: impl std::future::Future<Output = T>,
) -> Result<T, BackendError> {
    tokio::time::timeout(limit, fut).await.map_err(|_| {
        BackendError::new(
            ErrorKind::NetworkOffline,
            "the server did not respond in time",
        )
    })
}

/// Resolves `fut` unless the stop switch is flipped first.
pub(crate) async fn or_stop<T>(
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
pub(crate) fn strong_etag(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(ETAG)?.to_str().ok()?.trim();
    let quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
    quoted.then(|| value.to_owned())
}

pub(crate) fn ensure_identity_encoding(headers: &HeaderMap) -> Result<(), BackendError> {
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

pub(crate) fn status_failure(response: &reqwest::Response) -> BackendError {
    let wait = retry_after_header(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
    );
    status_error(response.status().as_u16(), wait)
}

/// Headers the caller supplied, scoped to the request's original origin.
pub(crate) fn with_caller_headers(
    mut req: reqwest::RequestBuilder,
    attempt: &Attempt,
    url: &Url,
    same_origin: bool,
) -> reqwest::RequestBuilder {
    for (name, value) in &attempt.request.headers {
        if same_origin || !origin_scoped(name) {
            req = req.header(name.as_str(), value.expose());
        }
    }
    // BRW-05 (ADR 0027): the session's cookies, re-evaluated for this exact URL (origin, path,
    // Secure, expiry). Another origin never gets them (and is stopped before it is requested).
    if let Some(session) = &attempt.request.session
        && let Some(cookie) = session.cookie_header(url.as_str(), now_ms())
    {
        req = req.header(reqwest::header::COOKIE, cookie.expose());
    }
    req
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A URL a scoped session may be used for: its own origin (scheme, host and effective port). An
/// authenticated attempt stops instead of following a URL outside it (ADR 0027).
pub(crate) fn session_scope_check(attempt: &Attempt, url: &Url) -> Result<(), BackendError> {
    match &attempt.request.session {
        Some(s) if dm_domain::cookies::Origin::parse(url.as_str()).as_ref() != Some(&s.origin) => {
            Err(BackendError::new(
                ErrorKind::AuthScopeUnsupported,
                "the signed-in download moved to another site or to an insecure connection; finish it in the browser",
            ))
        }
        _ => Ok(()),
    }
}

/// The first request: follows redirects manually and returns the final `200` and its URL.
async fn initial_request(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(reqwest::Response, Url, Permit), Ended> {
    let first = gate_url(&attempt.request.url, &config.scheme_gate)?;
    let origin = first.origin();
    let requested = first.clone();
    let mut url = first;
    let mut hops: u8 = 0;
    loop {
        // Each hop takes a slot for its own host first; a redirect drops the previous hop's
        // response and slot (end of this iteration) before the next hop asks.
        session_scope_check(attempt, &url)?;
        let permit = or_stop(stop, attempt.limiter.acquire(&url)).await??;
        let req = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
        let req = with_caller_headers(req, attempt, &url, url.origin() == origin);
        let response = or_stop(stop, within(attempt.read_timeout, req.send()))
            .await??
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
                // A loop or a very long chain is usually the site sending the client to a sign-in
                // or cookie check the browser would handle: a link problem with a way on, not an
                // invalid request (Add URL matrix, 2026-11-14).
                return Err(BackendError::new(
                    ErrorKind::LinkRefreshRequired,
                    "the site kept redirecting (often a sign-in or cookie check); open the link in the browser or get a direct link",
                )
                .into());
            }
            gate(&next, &config.scheme_gate)?;
            if matches!(next.scheme(), "ftp" | "sftp") {
                // F05 (ADR 0035): a web link never switches protocol on its own.
                return Err(BackendError::new(
                    ErrorKind::BackendCapabilityUnavailable,
                    "the web link redirects to an FTP or SFTP address; add that link itself",
                )
                .into());
            }
            drop(response);
            drop(permit);
            url = next;
            continue;
        }
        if status != reqwest::StatusCode::OK {
            return Err(status_failure(&response).into());
        }
        if let Some(e) = web_page_instead_of_file(response.headers(), &requested, &url) {
            return Err(e.into());
        }
        return Ok((response, url, permit));
    }
}

/// File types a web page never legitimately stands for. A link that names one of them and is
/// answered with HTML is a sign-in, expired-link or "your download starts…" page, not the file.
const NOT_A_PAGE: &[&str] = &[
    "7z", "aab", "apk", "appimage", "appx", "avi", "bin", "bz2", "cab", "deb", "dmg", "docx",
    "exe", "flac", "gz", "img", "iso", "jar", "m4a", "mkv", "mov", "mp3", "mp4", "msi", "msix",
    "ogg", "pdf", "pkg", "pptx", "rar", "rpm", "tar", "tgz", "wav", "webm", "xlsx", "xz", "zip",
    "zst",
];

fn file_extension(url: &Url) -> Option<String> {
    let last = url.path_segments()?.next_back()?;
    let (_, ext) = last.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

/// An HTML answer to a link that names a file of another kind, with no `Content-Disposition`
/// naming the attachment: refused instead of being saved under that file's name. A real page
/// (`.html`, an address without an extension) or an attachment the server names is unaffected.
pub(crate) fn web_page_instead_of_file(
    headers: &HeaderMap,
    requested: &Url,
    final_url: &Url,
) -> Option<BackendError> {
    let media = headers.get(CONTENT_TYPE)?.to_str().ok()?;
    let media = media.split(';').next()?.trim().to_ascii_lowercase();
    if media != "text/html" && media != "application/xhtml+xml" {
        return None;
    }
    if headers.contains_key(CONTENT_DISPOSITION) {
        return None;
    }
    let names_a_file = [requested, final_url]
        .into_iter()
        .filter_map(file_extension)
        .any(|ext| NOT_A_PAGE.contains(&ext.as_str()));
    names_a_file.then(page_instead_of_file)
}

fn page_instead_of_file() -> BackendError {
    BackendError::new(
        ErrorKind::LinkRefreshRequired,
        "the server sent a web page instead of the file (a sign-in, an expired link or a download page); open the link in the browser or get a fresh link",
    )
}

/// Whether the first body bytes are an HTML document (after an optional UTF-8 BOM and
/// whitespace). None of the [`NOT_A_PAGE`] formats can begin like this.
pub(crate) fn looks_like_html(first: &[u8]) -> bool {
    let body = first.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(first);
    let head: Vec<u8> = body
        .iter()
        .skip_while(|c| c.is_ascii_whitespace())
        .take(14)
        .map(u8::to_ascii_lowercase)
        .collect();
    head.starts_with(b"<!doctype html") || head.starts_with(b"<html")
}

fn names_a_file(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| NOT_A_PAGE.contains(&ext.to_ascii_lowercase().as_str()))
}

/// NB06: the first bytes written at offset 0 of a link that names a file (`.zip`, `.exe`, …).
/// A server that mislabels a sign-in or error page (`application/octet-stream`, or no type at
/// all) is caught by its content. As in NB04, an attachment the server names itself
/// (`Content-Disposition`) is the server's statement and is kept.
pub(crate) fn disguised_page(attempt: &Attempt, first: &[u8]) -> Option<BackendError> {
    if !looks_like_html(first) {
        return None;
    }
    let naming = lock(&attempt.naming).clone()?;
    if naming.content_disposition.is_some() {
        return None;
    }
    let requested = Url::parse(&attempt.request.url).ok();
    let final_name = naming.url_file_name.as_deref().is_some_and(names_a_file);
    let requested_name = requested
        .as_ref()
        .and_then(file_extension)
        .is_some_and(|ext| NOT_A_PAGE.contains(&ext.as_str()));
    (final_name || requested_name).then(page_instead_of_file)
}

/// Keeps the naming inputs of a full `200` for the service (B1.6). Only reported: the partial
/// file keeps its name whatever the server says.
fn record_naming(attempt: &Attempt, headers: &HeaderMap, url: &Url) {
    *lock(&attempt.naming) = Some(response_naming(headers, url));
}

/// Like [`record_naming`] for a validated `206` of the same representation (a resumed or
/// restored attempt has seen no `200` of its own); an earlier `200`'s inputs are kept.
pub(crate) fn record_naming_once(attempt: &Attempt, headers: &HeaderMap, url: &Url) {
    let mut naming = lock(&attempt.naming);
    if naming.is_none() {
        *naming = Some(response_naming(headers, url));
    }
}

fn response_naming(headers: &HeaderMap, url: &Url) -> ResponseNaming {
    let content_disposition = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .map(|v| v.as_bytes())
        .filter(|b| b.len() <= MAX_DISPOSITION_BYTES)
        .map(|b| match std::str::from_utf8(b) {
            Ok(text) => text.to_owned(),
            // Not UTF-8: field values are ISO-8859-1 octets (RFC 9110 §5.5).
            Err(_) => b.iter().map(|&c| char::from(c)).collect(),
        });
    let url_file_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned);
    ResponseNaming {
        content_disposition,
        url_file_name,
    }
}

/// Outcome of a resume request.
enum Resumed {
    /// A validated `206` continuing at the checkpoint offset, after `overlap` bytes that must
    /// equal the end of the partial file (ADR 0044; zero with a strong ETag).
    Continue(reqwest::Response, Permit, u64),
    /// A `200`: the full (possibly new) representation; the file restarts from zero.
    Restart(reqwest::Response, Identity, Permit),
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
    // ADR 0044: a strong ETag when there is one; otherwise a strong Last-Modified, with the last
    // confirmed bytes fetched again and compared before anything is appended.
    let (validator, overlap) = match (&cp.identity.etag, &cp.identity.last_modified) {
        (Some(etag), _) => (etag.clone(), 0),
        (None, Some(lm)) => (lm.clone(), cp.offset.min(OVERLAP_BYTES)),
        (None, None) => {
            return Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "no strong validator",
            )
            .into());
        }
    };
    let first = cp.offset.saturating_sub(overlap);
    let origin = gate_url(&attempt.request.url, &config.scheme_gate)?.origin();
    let req = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={first}-"))
        .header(IF_RANGE, validator);
    session_scope_check(attempt, &url)?;
    let req = with_caller_headers(req, attempt, &url, url.origin() == origin);
    let permit = or_stop(stop, attempt.limiter.acquire(&url)).await??;
    let response = or_stop(stop, within(attempt.read_timeout, req.send()))
        .await??
        .map_err(|e| transport_error(&e))?;
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {
            validate_partial_response(response.headers(), cp, first)?;
            record_naming_once(attempt, response.headers(), &url);
            Ok(Resumed::Continue(response, permit, overlap))
        }
        reqwest::StatusCode::OK => {
            ensure_identity_encoding(response.headers())?;
            record_naming(attempt, response.headers(), &url);
            let identity =
                Identity::of_response(response.headers(), response.content_length(), url);
            Ok(Resumed::Restart(response, identity, permit))
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
    first: u64,
) -> Result<(), BackendError> {
    let length = cp
        .identity
        .length
        .ok_or_else(|| BackendError::new(ErrorKind::BackendCapabilityUnavailable, "no length"))?;
    let last = length
        .checked_sub(1)
        .ok_or_else(|| BackendError::new(ErrorKind::InvalidRange, "empty representation"))?;
    if cp.identity.etag.is_some() || cp.identity.last_modified.is_none() {
        return validate_range_response(headers, cp.identity.etag.as_deref(), length, first, last);
    }
    // ADR 0044: no strong ETag; the exact Last-Modified of the original response.
    ensure_identity_encoding(headers)?;
    let now = headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    if now != cp.identity.last_modified.as_deref() {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file on the server changed (Last-Modified differs); nothing was appended",
        ));
    }
    validate_range_shape(headers, length, first, last)
}

/// Every check that must pass before a single byte of a `206` for `first..=last` of a
/// `total`-byte representation with strong ETag `etag` is written.
pub(crate) fn validate_range_response(
    headers: &HeaderMap,
    etag: Option<&str>,
    total: u64,
    first: u64,
    last: u64,
) -> Result<(), BackendError> {
    ensure_identity_encoding(headers)?;
    if etag.is_none() || strong_etag(headers).as_deref() != etag {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file on the server changed (ETag differs); nothing was appended",
        ));
    }
    validate_range_shape(headers, total, first, last)
}

/// `Content-Range` and `Content-Length` of a `206` for `first..=last` of `total` bytes.
fn validate_range_shape(
    headers: &HeaderMap,
    total: u64,
    first: u64,
    last: u64,
) -> Result<(), BackendError> {
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
    if range.total != total {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file length on the server changed; nothing was appended",
        ));
    }
    if range.first != first || range.last != last {
        return Err(BackendError::new(
            ErrorKind::InvalidRange,
            "the server returned a different byte range than requested; nothing was appended",
        ));
    }
    let span = last.checked_sub(first).and_then(|d| d.checked_add(1));
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
    // F05 (ADR 0035): ftp:// has its own transport and the same write path.
    if attempt
        .request
        .url
        .get(..6)
        .is_some_and(|s| s.eq_ignore_ascii_case("ftp://"))
    {
        return crate::ftp::fetch(config, attempt, stop).await;
    }
    if attempt
        .request
        .url
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("sftp://"))
    {
        return crate::sftp::fetch(config, attempt, stop).await;
    }
    match fetch_planned(client, config, attempt, stop).await {
        // A segment received a `200`: every worker has stopped and flushed. The representation is
        // not trusted for splicing, so the whole file restarts on one connection.
        Err(Ended::Restart) => {
            *lock(&attempt.seg_checkpoint) = None;
            let (response, identity, permit) = fresh_request(client, config, attempt, stop).await?;
            write_single(
                attempt,
                stop,
                http_body(response),
                identity,
                0,
                Sha256::new(),
                permit,
            )
            .await
        }
        other => other,
    }
}

/// The first request of a transfer and the identity its `200` establishes.
pub(crate) async fn fresh_request(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(reqwest::Response, Identity, Permit), Ended> {
    let (response, url, permit) = initial_request(client, config, attempt, stop).await?;
    ensure_identity_encoding(response.headers())?;
    record_naming(attempt, response.headers(), &url);
    let identity = Identity::of_response(response.headers(), response.content_length(), url);
    Ok((response, identity, permit))
}

async fn fetch_planned(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let single_resume = lock(&attempt.resume_from).take();
    if let Some(cp) = &single_resume
        && cp.identity.length == Some(cp.offset)
    {
        // Every byte was already confirmed (e.g. the service stopped between the last write and
        // finalization): nothing to fetch, and a `Range: bytes=<length>-` would only earn a 416.
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&attempt.part)
            .await
            .map_err(|e| io_error("cannot open partial file", &e))?;
        file.sync_all()
            .await
            .map_err(|e| io_error("cannot flush partial file", &e))?;
        let mut shared = lock(&attempt.shared);
        shared.completed = cp.offset;
        shared.total = cp.identity.length;
        return Ok(());
    }
    if let Some(cp) = single_resume {
        return match resume_request(client, config, attempt, &cp, stop).await? {
            Resumed::Continue(response, permit, overlap) => {
                let body = matching_overlap(attempt, stop, http_body(response), cp.offset, overlap)
                    .await?;
                write_single(
                    attempt,
                    stop,
                    body,
                    cp.identity,
                    cp.offset,
                    cp.hasher,
                    permit,
                )
                .await
            }
            Resumed::Restart(response, identity, permit) => {
                write_single(
                    attempt,
                    stop,
                    http_body(response),
                    identity,
                    0,
                    Sha256::new(),
                    permit,
                )
                .await
            }
        };
    }
    let segmented_resume = lock(&attempt.seg_resume_from).take();
    if let Some(cp) = segmented_resume {
        return segmented::run(client, config, attempt, cp, stop).await;
    }
    let (response, identity, permit) = fresh_request(client, config, attempt, stop).await?;
    if let Some(cp) = segmented::plan(&identity, attempt.request.max_connections) {
        // The probe established identity and length; its body is not used. Every byte comes from
        // a validated `206` per segment. The probe's connection and slot are given up first.
        drop(response);
        drop(permit);
        return segmented::run(client, config, attempt, cp, stop).await;
    }
    write_single(
        attempt,
        stop,
        http_body(response),
        identity,
        0,
        Sha256::new(),
        permit,
    )
    .await
}

/// ADR 0044: reads the first `overlap` bytes of a `Last-Modified`-validated `206` and compares
/// them with the partial file's bytes before `offset`. Equal: the rest of the body continues at
/// `offset`. Different: the file on the server changed, and nothing is appended.
async fn matching_overlap(
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
    mut body: Body,
    offset: u64,
    overlap: u64,
) -> Result<Body, Ended> {
    if overlap == 0 {
        return Ok(body);
    }
    let start = offset
        .checked_sub(overlap)
        .ok_or_else(|| BackendError::new(ErrorKind::Internal, "overlap before the file start"))?;
    let want = usize::try_from(overlap)
        .map_err(|_| BackendError::new(ErrorKind::Internal, "overlap too large"))?;
    let on_disk = {
        let part = attempt.part.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>, BackendError> {
            use std::io::{Read, Seek};
            let mut file =
                std::fs::File::open(&part).map_err(|e| io_error("cannot read partial file", &e))?;
            file.seek(std::io::SeekFrom::Start(start))
                .map_err(|e| io_error("cannot read partial file", &e))?;
            let mut buf = vec![0u8; want];
            file.read_exact(&mut buf)
                .map_err(|e| io_error("cannot read partial file", &e))?;
            Ok(buf)
        })
        .await
        .map_err(|_| BackendError::new(ErrorKind::Internal, "partial file check failed"))??
    };
    let mut received: Vec<u8> = Vec::with_capacity(want);
    let mut rest: Option<bytes::Bytes> = None;
    while received.len() < want {
        let chunk = match or_stop(stop, within(attempt.read_timeout, body.next())).await?? {
            None => {
                return Err(BackendError::new(
                    ErrorKind::InvalidRange,
                    "the connection ended before the resume point; nothing was appended",
                )
                .into());
            }
            Some(chunk) => chunk?,
        };
        let need = want.saturating_sub(received.len());
        if chunk.len() > need {
            received.extend_from_slice(chunk.get(..need).unwrap_or_default());
            rest = Some(chunk.slice(need..));
        } else {
            received.extend_from_slice(&chunk);
        }
    }
    if received != on_disk {
        return Err(BackendError::new(
            ErrorKind::RemoteChanged,
            "the file on the server changed (the bytes before the resume point differ); nothing was appended",
        )
        .into());
    }
    Ok(match rest {
        Some(rest) => Box::pin(futures_util::stream::iter([Ok(rest)]).chain(body)),
        None => body,
    })
}

/// A transfer's body as validated byte chunks: an HTTP response body, or FTP data (ftp.rs).
pub(crate) type Body =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, BackendError>> + Send>>;

/// The body of an HTTP response, with transport failures classified.
fn http_body(response: reqwest::Response) -> Body {
    Box::pin(
        response
            .bytes_stream()
            .map(|r| r.map_err(|e| transport_error(&e))),
    )
}

/// Streams one body into the partial file from `start` (single connection).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_single(
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
    mut body: Body,
    identity: Identity,
    start: u64,
    mut hasher: Sha256,
    // Held for the whole body: the connection's slot in the per-host budget.
    _permit: Permit,
) -> Result<(), Ended> {
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
    // Disk budget before the first body byte: the whole remaining length, or (unknown length) the
    // next sync window.
    let dir = &attempt.request.destination_dir;
    let still_needed = |written: u64| {
        identity
            .length
            .map_or(SYNC_BYTES, |total| total.saturating_sub(written))
    };
    config_space(attempt).ensure(dir, still_needed(start))?;

    let mut out = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);
    let mut written: u64 = start;
    let mut synced: u64 = start;
    // NB06: only a body written from offset 0 is checked, once, before its first byte is written.
    let mut sniffed = start > 0;
    let mut last_sync = tokio::time::Instant::now();
    loop {
        let next_chunk = match or_stop(stop, within(attempt.read_timeout, body.next())).await {
            Ok(Ok(next_chunk)) => next_chunk,
            Ok(Err(timed_out)) => {
                // Bytes already accepted passed every validation: keep them on disk.
                let _ = out.flush().await;
                return Err(timed_out.into());
            }
            Err(ended) => {
                // Stopping: flush and sync, then record what is confirmed on disk, so the partial
                // file matches the reported progress and the (durable) resume point.
                if out.flush().await.is_ok() && out.get_ref().sync_data().await.is_ok() {
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
                return Err(e.into());
            }
            Some(Ok(chunk)) => chunk,
        };
        if !sniffed {
            sniffed = true;
            if let Some(e) = disguised_page(attempt, &chunk) {
                return Err(e.into());
            }
        }
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
        fault::storage_full("enospc_write", synced > start)
            .map_err(|e| io_error("cannot write partial file", &e))?;
        match attempt.rates.part_limit() {
            // Unlimited (CORE-06): the whole chunk in one write, as before.
            None => {
                out.write_all(&chunk)
                    .await
                    .map_err(|e| io_error("cannot write partial file", &e))?;
                hasher.update(&chunk);
                written = next;
                lock(&attempt.shared).completed = written;
            }
            // Limited: bounded parts, each charged before it is written, so progress, the hash and
            // every checkpoint count only bytes actually written (ADR 0024).
            Some(limit) => {
                let mut rest: &[u8] = &chunk;
                while !rest.is_empty() {
                    let part = attempt.rates.part_limit().unwrap_or(limit).min(rest.len());
                    if or_stop(stop, attempt.rates.charge(part as u64))
                        .await
                        .is_err()
                    {
                        if out.flush().await.is_ok() && out.get_ref().sync_data().await.is_ok() {
                            *lock(&attempt.checkpoint) = Some(Checkpoint {
                                identity: identity.clone(),
                                offset: written,
                                hasher: hasher.clone(),
                            });
                        }
                        return Err(Ended::Stopped);
                    }
                    out.write_all(&rest[..part])
                        .await
                        .map_err(|e| io_error("cannot write partial file", &e))?;
                    hasher.update(&rest[..part]);
                    written = written.saturating_add(part as u64);
                    lock(&attempt.shared).completed = written;
                    rest = &rest[part..];
                }
            }
        }
        if written.saturating_sub(synced) >= SYNC_BYTES || last_sync.elapsed() >= SYNC_EVERY {
            out.flush()
                .await
                .map_err(|e| io_error("cannot write partial file", &e))?;
            fault::storage_full("enospc_sync", synced > start)
                .map_err(|e| io_error("cannot flush partial file", &e))?;
            out.get_ref()
                .sync_data()
                .await
                .map_err(|e| io_error("cannot flush partial file", &e))?;
            *lock(&attempt.checkpoint) = Some(Checkpoint {
                identity: identity.clone(),
                offset: written,
                hasher: hasher.clone(),
            });
            synced = written;
            last_sync = tokio::time::Instant::now();
            // Re-check with what is still needed: other writers share the volume.
            config_space(attempt).ensure(dir, still_needed(written))?;
        }
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
                last_modified: None,
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
    fn naming_inputs_are_read_as_utf8_or_latin1_and_never_include_the_query() {
        let url = Url::parse("https://h/dir/f%C3%BCr%20x.bin?sig=SECRET#frag").unwrap();
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_bytes(b"attachment; filename=\"caf\xE9.txt\"")
                .unwrap(),
        );
        let n = response_naming(&h, &url);
        assert_eq!(
            n.content_disposition.as_deref(),
            Some("attachment; filename=\"café.txt\"")
        );
        assert_eq!(n.url_file_name.as_deref(), Some("f%C3%BCr%20x.bin"));
        let root = response_naming(&HeaderMap::new(), &Url::parse("https://h/").unwrap());
        assert_eq!(root, ResponseNaming::default());
    }

    #[test]
    fn html_is_recognised_by_its_first_bytes_and_files_are_not() {
        for page in [
            &b"<!doctype html><p>x"[..],
            b"<!DOCTYPE HTML>",
            b"\xEF\xBB\xBF \r\n\t<html lang=en>",
            b"<HTML>",
        ] {
            assert!(looks_like_html(page), "{page:?}");
        }
        for file in [
            &b"PK\x03\x04"[..],
            b"MZ\x90\x00",
            b"%PDF-1.7",
            b"<?xml version=1.0?>",
            b"<!doctype htm",
            b"",
            b"   ",
        ] {
            assert!(!looks_like_html(file), "{file:?}");
        }
        assert!(names_a_file("setup.EXE") && names_a_file("a.b.zip"));
        assert!(!names_a_file("page.html") && !names_a_file("download"));
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
        assert!(validate_partial_response(&headers(&good), &cp, cp.offset).is_ok());
        let kind = |h: &[(&'static str, &str)]| {
            validate_partial_response(&headers(h), &cp, cp.offset)
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
}
