//! The transfer task: TLS client, manual redirects, response validation, streaming to disk and
//! publishing the finished file.

use std::path::Path;
use std::sync::Arc;

use dm_backend_api::{BackendError, TransferState, require_capability};
use dm_domain::redact::is_sensitive_header;
use dm_domain::{Capability, CapabilityReport, ErrorKind};
use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, LOCATION, RETRY_AFTER};
use tokio::io::AsyncWriteExt;
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

async fn fetch(
    client: &reqwest::Client,
    config: &HttpConfig,
    attempt: &Attempt,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Ended> {
    let first = gate_url(&attempt.request.url, &config.scheme_gate)?;
    let origin = first.origin();
    let mut url = first;
    let mut hops: u8 = 0;
    let response = loop {
        let same_origin = url.origin() == origin;
        let mut req = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
        for (name, value) in &attempt.request.headers {
            if same_origin || !origin_scoped(name) {
                req = req.header(name.as_str(), value.expose());
            }
        }
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
            let wait = retry_after_header(
                response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok()),
            );
            return Err(status_error(status.as_u16(), wait).into());
        }
        break response;
    };

    if let Some(encoding) = response.headers().get(CONTENT_ENCODING)
        && !encoding
            .to_str()
            .is_ok_and(|e| e.trim().eq_ignore_ascii_case("identity"))
    {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "the server sent an encoded body although identity was requested",
        )
        .into());
    }
    let expected = response.content_length();
    {
        let mut shared = lock(&attempt.shared);
        shared.state = TransferState::Active;
        shared.total = expected;
        shared.completed = 0;
    }

    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&attempt.part)
        .await
        .map_err(|e| io_error("cannot open partial file", &e))?;
    let mut out = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);
    let mut body = response.bytes_stream();
    let mut written: u64 = 0;
    loop {
        let next_chunk = match or_stop(stop, body.next()).await {
            Ok(next_chunk) => next_chunk,
            Err(ended) => {
                // Stopping: the bytes already counted reach the partial file before the stop is
                // acknowledged, so the file matches the reported progress.
                let _ = out.flush().await;
                return Err(ended);
            }
        };
        let chunk = match next_chunk {
            None => break,
            Some(Err(e)) => return Err(transport_error(&e).into()),
            Some(Ok(chunk)) => chunk,
        };
        let next = u64::try_from(chunk.len())
            .ok()
            .and_then(|n| written.checked_add(n))
            .ok_or_else(|| BackendError::new(ErrorKind::Internal, "byte count overflow"))?;
        if expected.is_some_and(|total| next > total) {
            return Err(BackendError::new(
                ErrorKind::InvalidRange,
                "the server sent more bytes than it announced",
            )
            .into());
        }
        out.write_all(&chunk)
            .await
            .map_err(|e| io_error("cannot write partial file", &e))?;
        written = next;
        lock(&attempt.shared).completed = written;
    }
    if let Some(total) = expected
        && written != total
    {
        return Err(BackendError::new(
            ErrorKind::NetworkOffline,
            format!("connection closed after {written} of {total} bytes"),
        )
        .into());
    }
    out.flush()
        .await
        .map_err(|e| io_error("cannot write partial file", &e))?;
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
