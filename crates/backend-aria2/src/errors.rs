//! Classification of aria2 errors into stable [`ErrorKind`]s.
//!
//! aria2's `errorMessage` echoes the request URL (including signed-query tokens), so it is never
//! forwarded. Only two facts are extracted from it (B0.4):
//! - the three-digit HTTP status for errorCode 22 ("The response status is not successful.
//!   status=NNN"), which aria2 uses for 403/410/429/5xx alike;
//! - whether errorCode 1 was an "SSL/TLS handshake failure".
//!
//! The resulting `detail` is built from numbers only.

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;

/// Extracts `NNN` from the last `status=NNN` fragment. The message starts with the request URL,
/// whose query may itself contain `status=`, so the first occurrence is not trustworthy.
fn http_status(message: &str) -> Option<u16> {
    let at = message.rfind("status=")?;
    let digits: String = message
        .get(at.saturating_add(7)..)?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (digits.len() == 3).then(|| digits.parse().ok()).flatten()
}

/// Maps an aria2 error code (aria2c manual, "EXIT STATUS"), refined by the message facts above.
pub fn classify(code: u32, message: Option<&str>) -> BackendError {
    let msg = message.unwrap_or_default();
    let (kind, detail) = match code {
        2 | 6 | 19 => (
            ErrorKind::NetworkOffline,
            format!("aria2 error code {code}"),
        ),
        3 | 4 => (
            ErrorKind::LinkRefreshRequired,
            "resource not found (HTTP 404)".to_owned(),
        ),
        8 => (
            ErrorKind::RangeUnsupported,
            format!("aria2 error code {code}"),
        ),
        9 => (ErrorKind::DiskFull, format!("aria2 error code {code}")),
        13 => (
            ErrorKind::InvalidRequest,
            "target file already exists".to_owned(),
        ),
        22 => match http_status(msg) {
            Some(s @ (401 | 407)) => (
                ErrorKind::AuthRequired,
                format!("authorization required (HTTP {s})"),
            ),
            Some(403) => (
                ErrorKind::LinkRefreshRequired,
                "server refused access (HTTP 403); the link may have expired or need sign-in"
                    .to_owned(),
            ),
            Some(s @ (404 | 410)) => (
                ErrorKind::LinkRefreshRequired,
                format!("resource no longer available (HTTP {s})"),
            ),
            // A failed precondition (If-Match) means the representation changed (B0.4.1).
            Some(412) => (
                ErrorKind::RemoteChanged,
                "the file on the server changed (HTTP 412 precondition failed)".to_owned(),
            ),
            Some(s @ (408 | 429)) => (
                ErrorKind::ServerBusy,
                format!("server asked to slow down (HTTP {s})"),
            ),
            Some(s @ 500..=599) => (ErrorKind::ServerBusy, format!("server error (HTTP {s})")),
            Some(s) => (
                ErrorKind::InvalidRequest,
                format!("unexpected HTTP status {s}"),
            ),
            // On a retried or resumed request aria2 reports only "No URI available." and drops the
            // status (B0.4.1 M02, M07), so a 412, 403 or 5xx there cannot be told apart.
            None => (
                ErrorKind::InvalidRange,
                "the server rejected a resumed or retried request; the engine did not report the HTTP status".to_owned(),
            ),
        },
        24 => (
            ErrorKind::AuthRequired,
            "authorization failed (HTTP 401)".to_owned(),
        ),
        29 => (
            ErrorKind::ServerBusy,
            "server overloaded (HTTP 503); Retry-After is not honoured by the engine".to_owned(),
        ),
        32 => (
            ErrorKind::ChecksumMismatch,
            format!("aria2 error code {code}"),
        ),
        // Over RPC an interrupted single-try transfer surfaces only as "No URI available." (the
        // console-only "Got EOF from the server" is not exposed), so the specific cause is unknown.
        1 if msg.contains("No URI available") => (
            ErrorKind::NetworkOffline,
            "transfer interrupted (connection closed or network error); the engine gave no specific reason"
                .to_owned(),
        ),
        1 if msg.contains("Got EOF from the server") => (
            ErrorKind::NetworkOffline,
            "connection closed before the whole file arrived".to_owned(),
        ),
        1 if msg.contains("SSL/TLS handshake failure") => (
            ErrorKind::TlsHandshakeFailed,
            "TLS handshake failed (certificate not trusted or invalid)".to_owned(),
        ),
        _ => (ErrorKind::Internal, format!("aria2 error code {code}")),
    };
    BackendError::new(kind, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "SYNTHETIC-SECRET-errors";

    fn msg(status: u16) -> String {
        format!(
            "URI=http://h/f?token={SECRET} -> [HttpSkipResponseCommand.cc:239] errorCode=22 The response status is not successful. status={status}"
        )
    }

    #[test]
    fn http_statuses_behind_code_22_are_distinguished() {
        assert_eq!(classify(22, Some(&msg(401))).kind, ErrorKind::AuthRequired);
        assert_eq!(
            classify(22, Some(&msg(403))).kind,
            ErrorKind::LinkRefreshRequired
        );
        assert_eq!(
            classify(22, Some(&msg(410))).kind,
            ErrorKind::LinkRefreshRequired
        );
        assert_eq!(classify(22, Some(&msg(429))).kind, ErrorKind::ServerBusy);
        assert_eq!(classify(22, Some(&msg(412))).kind, ErrorKind::RemoteChanged);
        assert_eq!(classify(22, Some(&msg(500))).kind, ErrorKind::ServerBusy);
        assert_eq!(
            classify(22, Some(&msg(418))).kind,
            ErrorKind::InvalidRequest
        );
        assert_eq!(
            classify(22, Some("no status here")).kind,
            ErrorKind::InvalidRange
        );
        assert_eq!(classify(22, None).kind, ErrorKind::InvalidRange);
    }

    #[test]
    fn a_status_parameter_in_the_url_does_not_change_the_classification() {
        let m = "URI=http://h/f?status=200&x=1 -> [HttpSkipResponseCommand.cc:239] errorCode=22 The response status is not successful. status=403";
        assert_eq!(classify(22, Some(m)).kind, ErrorKind::LinkRefreshRequired);
    }

    #[test]
    fn details_never_contain_the_engine_message() {
        for code in [1, 3, 22, 24, 29, 99] {
            let e = classify(code, Some(&msg(403)));
            assert!(
                !e.detail.contains(SECRET) && !e.detail.contains("http://"),
                "{}",
                e.detail
            );
        }
    }

    #[test]
    fn tls_and_busy_and_known_codes() {
        let tls = "errorCode=1 SSL/TLS handshake failure: Error: The certificate chain was issued by an authority that is not trusted.";
        assert_eq!(classify(1, Some(tls)).kind, ErrorKind::TlsHandshakeFailed);
        assert_eq!(
            classify(1, Some("something else")).kind,
            ErrorKind::Internal
        );
        let eof = "[DownloadCommand.cc:234] errorCode=1 Got EOF from the server.";
        assert_eq!(classify(1, Some(eof)).kind, ErrorKind::NetworkOffline);
        assert_eq!(
            classify(1, Some("No URI available.")).kind,
            ErrorKind::NetworkOffline
        );
        assert_eq!(classify(29, None).kind, ErrorKind::ServerBusy);
        assert_eq!(classify(9, None).kind, ErrorKind::DiskFull);
        assert_eq!(classify(24, None).kind, ErrorKind::AuthRequired);
        assert_eq!(classify(13, None).kind, ErrorKind::InvalidRequest);
    }
}
