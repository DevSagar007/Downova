//! Classification of HTTP statuses and transport failures into stable [`ErrorKind`]s.
//!
//! Details are built from status numbers and fixed phrases only. `reqwest::Error`'s `Display`
//! includes the request URL (with any signed-query token), so it is never used.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use dm_domain::retry::parse_retry_after;

/// Maps a non-success, non-redirect final status. `retry_after` is attached for statuses where a
/// server-requested wait is meaningful (408, 429, 5xx); the service decides whether to retry.
pub fn status_error(status: u16, retry_after: Option<Duration>) -> BackendError {
    let (kind, detail) = match status {
        401 | 407 => (
            ErrorKind::AuthRequired,
            format!("authorization required (HTTP {status})"),
        ),
        403 => (
            ErrorKind::LinkRefreshRequired,
            "server refused access (HTTP 403); the link may have expired or need sign-in"
                .to_owned(),
        ),
        404 | 410 => (
            ErrorKind::LinkRefreshRequired,
            format!("resource no longer available (HTTP {status})"),
        ),
        412 => (
            ErrorKind::RemoteChanged,
            "the file on the server changed (HTTP 412 precondition failed)".to_owned(),
        ),
        416 => (
            ErrorKind::InvalidRange,
            "the server rejected the requested range (HTTP 416)".to_owned(),
        ),
        206 => (
            ErrorKind::InvalidRange,
            "partial content (HTTP 206) to a request that asked for the whole file".to_owned(),
        ),
        408 | 429 => (
            ErrorKind::ServerBusy,
            format!("server asked to slow down (HTTP {status})"),
        ),
        500..=599 => (
            ErrorKind::ServerBusy,
            format!("server error (HTTP {status})"),
        ),
        _ => (
            ErrorKind::InvalidRequest,
            format!("unexpected HTTP status {status}"),
        ),
    };
    let error = BackendError::new(kind, detail);
    match retry_after {
        Some(wait) if kind == ErrorKind::ServerBusy => error.with_retry_after(wait),
        _ => error,
    }
}

/// Parses a `Retry-After` header value against the current clock.
pub fn retry_after_header(value: Option<&str>) -> Option<Duration> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    value.and_then(|v| parse_retry_after(v, now))
}

/// Classifies a transport-level failure without ever formatting the error itself.
pub fn transport_error(e: &reqwest::Error) -> BackendError {
    if let Some(tls) = find_rustls_error(e) {
        return BackendError::new(ErrorKind::TlsHandshakeFailed, tls_detail(tls));
    }
    let detail = if e.is_timeout() {
        "the server did not respond in time"
    } else if e.is_connect() {
        "could not connect to the server"
    } else if e.is_body() || e.is_decode() {
        "the connection failed while receiving the file"
    } else {
        "network error"
    };
    BackendError::new(ErrorKind::NetworkOffline, detail)
}

fn find_rustls_error<'a>(e: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = current {
        if let Some(tls) = err.downcast_ref::<rustls::Error>() {
            return Some(tls);
        }
        // A custom io::Error hides its payload from source(): descend into it explicitly
        // (hyper-rustls nests rustls::Error inside two io::Errors).
        current = match err
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
        {
            Some(inner) => Some(inner as &(dyn std::error::Error + 'static)),
            None => err.source(),
        };
    }
    None
}

fn tls_detail(e: &rustls::Error) -> String {
    use rustls::CertificateError as C;
    let reason = match e {
        rustls::Error::InvalidCertificate(c) => match c {
            C::UnknownIssuer => "issued by an untrusted authority",
            C::NotValidForName | C::NotValidForNameContext { .. } => "not valid for this host name",
            C::Expired | C::ExpiredContext { .. } => "expired",
            C::NotValidYet | C::NotValidYetContext { .. } => "not valid yet",
            C::Revoked => "revoked",
            C::BadSignature => "bad signature",
            _ => "rejected",
        },
        rustls::Error::AlertReceived(_) => "the server ended the handshake",
        _ => "handshake failed",
    };
    format!("TLS certificate or handshake rejected: {reason}")
}

/// Maps a local file error; never includes the path.
pub fn io_error(what: &str, e: &std::io::Error) -> BackendError {
    let kind = match e.kind() {
        // A full volume and an exhausted per-user quota are the same to the user (ADV05).
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => ErrorKind::DiskFull,
        std::io::ErrorKind::NotFound => ErrorKind::VolumeMissing,
        _ => ErrorKind::Internal,
    };
    BackendError::new(kind, format!("{what}: {}", e.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_stable_kinds() {
        let k = |s| status_error(s, None).kind;
        assert_eq!(k(401), ErrorKind::AuthRequired);
        assert_eq!(k(407), ErrorKind::AuthRequired);
        assert_eq!(k(403), ErrorKind::LinkRefreshRequired);
        assert_eq!(k(404), ErrorKind::LinkRefreshRequired);
        assert_eq!(k(410), ErrorKind::LinkRefreshRequired);
        assert_eq!(k(412), ErrorKind::RemoteChanged);
        assert_eq!(k(416), ErrorKind::InvalidRange);
        assert_eq!(k(206), ErrorKind::InvalidRange);
        assert_eq!(k(429), ErrorKind::ServerBusy);
        assert_eq!(k(503), ErrorKind::ServerBusy);
        assert_eq!(k(500), ErrorKind::ServerBusy);
        assert_eq!(k(418), ErrorKind::InvalidRequest);
    }

    #[test]
    fn retry_after_travels_only_with_busy_statuses() {
        let wait = Some(Duration::from_secs(30));
        assert_eq!(status_error(503, wait).retry_after, wait);
        assert_eq!(status_error(429, wait).retry_after, wait);
        assert_eq!(status_error(403, wait).retry_after, None);
        assert_eq!(retry_after_header(Some("7")), Some(Duration::from_secs(7)));
        assert_eq!(retry_after_header(Some("soon")), None);
        assert_eq!(retry_after_header(None), None);
    }
}
