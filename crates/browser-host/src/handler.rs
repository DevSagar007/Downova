//! One native-messaging request → one response (B0.6).
//!
//! The request is parsed against the closed contract (`NativeHostRequest`: `hello` and `add`
//! only), the URL is checked (http/https, no userinfo, no control characters, bounded length),
//! and `add` is forwarded as `downloads.v1.add` over the private IPC endpoint with the
//! extension's idempotency key and destination profile `downloads`. Nothing else is forwarded.
//!
//! Outcomes: `accepted` (the service returned a job), `rejected` (unsupported request or the
//! service refused it), `unavailable` (no compatible service; nothing was sent), `uncertain` (the
//! request was sent but no acknowledgement arrived in time — the caller must retry with the same
//! key, which returns the same job while the service keeps running).

use std::path::PathBuf;
use std::time::Duration;

use dm_domain::api::{
    AddDownloadParams, DownloadSnapshot, DownloadSource, NativeHostOutcome, NativeHostRequest,
    NativeHostResponse, methods,
};
use dm_domain::{ErrorData, ErrorKind};
use dm_platform::ipc::{CallError, Client};
use dm_platform::launch::{self, Readiness};
use serde_json::Value;

pub use dm_domain::validate::MAX_URL;
/// Destination profile used for browser hand-offs.
pub const PROFILE: &str = "downloads";

/// Where and how to reach the service.
#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    /// Bound on the `downloads.v1.add` acknowledgement.
    pub call_timeout: Duration,
}

fn response(
    request_id: Option<&str>,
    outcome: NativeHostOutcome,
    job: Option<DownloadSnapshot>,
    error: Option<ErrorData>,
) -> NativeHostResponse {
    NativeHostResponse {
        v: 1,
        request_id: request_id.map(str::to_owned),
        outcome,
        job,
        hello: None,
        error,
    }
}

fn err(kind: ErrorKind, detail: &str) -> Option<ErrorData> {
    Some(kind.to_error_data(detail))
}

pub use dm_domain::validate::check_url;
use dm_domain::validate::is_token as token;

/// Message shown when no service runs. The host never starts the service itself: on Windows a
/// child of the host would inherit the browser's pipe handles and the browser's job object (which
/// may kill it when the host exits), and neither can be prevented without unsafe code. Starting
/// the service is left to the Downova app (or, later, login autostart).
pub const NOT_RUNNING: &str =
    "the Downova download service is not running; open the Downova app, then try again";

/// Requires a compatible, already running service.
async fn ready(config: &Config) -> Result<dm_domain::api::HelloResult, ErrorData> {
    match launch::probe(&config.data_dir).await {
        Readiness::Ready(hello) => Ok(hello),
        Readiness::Incompatible(v) => Err(ErrorKind::ServiceUnavailable.to_error_data(format!(
            "an incompatible Downova service (API v{v}) is running"
        ))),
        Readiness::NotRunning(_) => Err(ErrorKind::ServiceUnavailable.to_error_data(NOT_RUNNING)),
    }
}

/// Handles one request body (already de-framed).
pub async fn handle(body: &[u8], config: &Config) -> NativeHostResponse {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return response(
            None,
            NativeHostOutcome::Rejected,
            None,
            err(ErrorKind::InvalidRequest, "the request is not valid JSON"),
        );
    };
    // Echo the id only when it is well formed.
    let echo = value
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| token(id, 8, 64))
        .map(str::to_owned);
    let request: NativeHostRequest = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(_) => {
            return response(
                echo.as_deref(),
                NativeHostOutcome::Rejected,
                None,
                err(
                    ErrorKind::InvalidRequest,
                    "unsupported request (only hello and add are accepted)",
                ),
            );
        }
    };
    let id = request.request_id().to_owned();
    if request.version() != 1 || !token(&id, 8, 64) {
        return response(
            echo.as_deref(),
            NativeHostOutcome::Rejected,
            None,
            err(
                ErrorKind::InvalidRequest,
                "unsupported protocol version or request id",
            ),
        );
    }
    match request {
        NativeHostRequest::Hello { .. } => match ready(config).await {
            Ok(hello) => NativeHostResponse {
                hello: Some(hello),
                ..response(Some(&id), NativeHostOutcome::ServiceReady, None, None)
            },
            Err(e) => response(Some(&id), NativeHostOutcome::Unavailable, None, Some(e)),
        },
        NativeHostRequest::Add {
            url,
            idempotency_key,
            ..
        } => {
            if let Err(why) = check_url(&url) {
                return response(
                    Some(&id),
                    NativeHostOutcome::Rejected,
                    None,
                    err(ErrorKind::InvalidRequest, why),
                );
            }
            if !token(&idempotency_key, 8, 128) {
                return response(
                    Some(&id),
                    NativeHostOutcome::Rejected,
                    None,
                    err(ErrorKind::InvalidRequest, "invalid idempotency key"),
                );
            }
            add(&id, url, idempotency_key, config).await
        }
    }
}

async fn add(id: &str, url: String, key: String, config: &Config) -> NativeHostResponse {
    if let Err(e) = ready(config).await {
        return response(Some(id), NativeHostOutcome::Unavailable, None, Some(e));
    }
    // Nothing has been sent yet: a failure to connect is "unavailable", not "uncertain".
    let mut client = match tokio::time::timeout(
        launch::HELLO_TIMEOUT,
        Client::connect(&config.data_dir),
    )
    .await
    {
        Ok(Ok(client)) => client,
        _ => {
            return response(
                Some(id),
                NativeHostOutcome::Unavailable,
                None,
                err(
                    ErrorKind::ServiceUnavailable,
                    "the download service is not reachable",
                ),
            );
        }
    };
    let params = AddDownloadParams {
        idempotency_key: key,
        source: DownloadSource { url },
        destination_profile_id: PROFILE.to_owned(),
        expected_size_bytes: None,
        expected_sha256: None,
    };
    let Ok(params) = serde_json::to_value(&params) else {
        return response(
            Some(id),
            NativeHostOutcome::Rejected,
            None,
            err(ErrorKind::Internal, "could not encode the request"),
        );
    };
    let uncertain = |why: &str| {
        response(
            Some(id),
            NativeHostOutcome::Uncertain,
            None,
            Some(ErrorData {
                kind: ErrorKind::ServiceUnavailable,
                retryable: true,
                next_action: dm_domain::NextAction::Retry,
                detail: Some(format!(
                    "{why}; the download may or may not have been added — retry the same request"
                )),
            }),
        )
    };
    match tokio::time::timeout(config.call_timeout, client.call(methods::ADD, params)).await {
        Err(_) => uncertain(&format!(
            "no acknowledgement from the download service within {} s",
            config.call_timeout.as_secs()
        )),
        Ok(Err(CallError::Unavailable(_))) => uncertain("the connection to the service was lost"),
        Ok(Err(CallError::Rpc(rpc))) => {
            let data = rpc.data.unwrap_or_else(|| {
                ErrorKind::Internal.to_error_data("the service refused the request")
            });
            let outcome = match data.kind {
                ErrorKind::BackendCapabilityUnavailable | ErrorKind::ServiceUnavailable => {
                    NativeHostOutcome::Unavailable
                }
                _ => NativeHostOutcome::Rejected,
            };
            response(Some(id), outcome, None, Some(data))
        }
        Ok(Ok(result)) => match serde_json::from_value::<DownloadSnapshot>(result) {
            Ok(job) => response(Some(id), NativeHostOutcome::Accepted, Some(job), None),
            Err(_) => uncertain("the service sent a malformed acknowledgement"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &std::path::Path) -> Config {
        Config {
            data_dir: dir.to_owned(),
            call_timeout: Duration::from_secs(2),
        }
    }

    #[test]
    fn urls_are_checked_syntactically() {
        for ok in [
            "https://example.org/f.zip",
            "HTTP://Example.org:8080/a?b=c#d",
            "https://[::1]:443/x",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "ftp://h/f",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:,x",
            "blob:https://h/1",
            "https://user:pw@h/f",
            "https://user@h/f",
            "https:///f",
            "https://h/f g",
            "https://h/f\u{0}",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        assert!(check_url(&format!("https://h/{}", "a".repeat(MAX_URL))).is_err());
        // An @ after the authority is part of the path, not credentials.
        assert!(check_url("https://h/a@b").is_ok());
    }

    #[tokio::test]
    async fn malformed_and_unsupported_requests_are_rejected_without_contacting_a_service() {
        let dir = tempfile::tempdir().unwrap();
        let c = config(dir.path());
        for (body, echoed) in [
            (&b"not json"[..], None),
            (br#"{"v":1,"request_id":"req_0000001","op":"rpc","method":"service.v1.shutdown"}"#, Some("req_0000001")),
            (br#"{"v":2,"request_id":"req_0000002","op":"hello"}"#, Some("req_0000002")),
            (br#"{"v":1,"request_id":"x","op":"hello"}"#, None),
            (br#"{"v":1,"request_id":"req_0000003","op":"add","url":"https://h/f","idempotency_key":"ext-00000001","cookies":"s=1"}"#, Some("req_0000003")),
            (br#"{"v":1,"request_id":"req_0000004","op":"add","url":"https://u:p@h/f","idempotency_key":"ext-00000001"}"#, Some("req_0000004")),
            (br#"{"v":1,"request_id":"req_0000005","op":"add","url":"https://h/f","idempotency_key":"bad key!"}"#, Some("req_0000005")),
        ] {
            let r = handle(body, &c).await;
            assert_eq!(r.outcome, NativeHostOutcome::Rejected, "{}", String::from_utf8_lossy(body));
            assert_eq!(r.request_id.as_deref(), echoed);
            assert!(r.job.is_none());
        }
    }

    #[tokio::test]
    async fn service_down_without_an_executable_is_unavailable_not_uncertain() {
        let dir = tempfile::tempdir().unwrap();
        let r = handle(
            br#"{"v":1,"request_id":"req_0000010","op":"add","url":"https://h/f","idempotency_key":"ext-00000010"}"#,
            &config(dir.path()),
        )
        .await;
        assert_eq!(r.outcome, NativeHostOutcome::Unavailable);
        assert_eq!(r.error.unwrap().kind, ErrorKind::ServiceUnavailable);
        let r = handle(
            br#"{"v":1,"request_id":"req_0000011","op":"hello"}"#,
            &config(dir.path()),
        )
        .await;
        assert_eq!(r.outcome, NativeHostOutcome::Unavailable);
    }
}
