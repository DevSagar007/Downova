//! Download commands (B2.1): the only path from the UI to `downloads.v1.*`.
//!
//! Five fixed operations — list, add, pause, resume, cancel — each validated here before anything
//! is sent, forwarded over the private IPC endpoint with the shared `dm-platform` client, and
//! answered with the contract's own types. There is no generic RPC forwarding: method names are
//! constants, add always uses destination profile `downloads`, and no file content, filesystem
//! path, database or engine detail crosses this boundary.
//!
//! Failures keep the service's structured `ErrorData` and say what the UI may assume:
//! - `unavailable`: nothing reached the service (no endpoint, connection refused or timed out);
//! - `uncertain`: a mutation was sent but no acknowledgement arrived — it may or may not have
//!   happened, so an add must be retried with the **same** idempotency key, never a new one;
//! - `rejected`: the service (or the argument checks here) refused it.

use std::path::Path;
use std::time::Duration;

use dm_domain::api::{
    AddDownloadParams, DownloadSnapshot, DownloadSource, JobRef, ListDownloadsResult, methods,
};
use dm_domain::validate::{check_url, is_idempotency_key};
use dm_domain::{ErrorData, ErrorKind, JobId, NextAction};
use dm_platform::ipc::{CallError, Client};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Destination profile of every download added from the desktop (the same as the browser host).
pub const PROFILE: &str = "downloads";
/// Bound for reaching the service before anything is sent.
pub const CONNECT_TIMEOUT: Duration = dm_platform::launch::HELLO_TIMEOUT;
/// Bound for a list call (the UI polls; a slow answer is simply retried by the next poll).
pub const LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound for an acknowledgement of a mutation. Cancel waits for the engine to stop.
pub const MUTATION_TIMEOUT: Duration = Duration::from_secs(15);

/// What the UI may assume about a failed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Unavailable,
    Uncertain,
    Rejected,
}

/// The error a command returns to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BridgeError {
    pub outcome: Outcome,
    pub error: ErrorData,
}

impl BridgeError {
    fn unavailable(detail: &str) -> Self {
        Self {
            outcome: Outcome::Unavailable,
            error: ErrorKind::ServiceUnavailable.to_error_data(detail),
        }
    }

    pub(crate) fn no_data_dir() -> Self {
        Self::unavailable("no per-user data directory")
    }

    /// A refused argument (nothing was sent).
    pub(crate) fn invalid_argument(detail: &str) -> Self {
        Self::invalid(detail)
    }

    /// A local failure of the shell itself.
    pub(crate) fn internal(detail: &str) -> Self {
        Self {
            outcome: Outcome::Rejected,
            error: ErrorKind::Internal.to_error_data(detail),
        }
    }

    fn invalid(detail: &str) -> Self {
        Self {
            outcome: Outcome::Rejected,
            error: ErrorKind::InvalidRequest.to_error_data(detail),
        }
    }

    fn uncertain(detail: &str) -> Self {
        Self {
            outcome: Outcome::Uncertain,
            error: ErrorData {
                kind: ErrorKind::ServiceUnavailable,
                retryable: true,
                next_action: NextAction::Retry,
                detail: Some(format!(
                    "{detail}; it may or may not have been applied — repeat the same request"
                )),
            },
        }
    }
}

/// The three job commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Pause,
    Resume,
    Cancel,
}

impl Control {
    fn method(self) -> &'static str {
        match self {
            Control::Pause => methods::PAUSE,
            Control::Resume => methods::RESUME,
            Control::Cancel => methods::CANCEL,
        }
    }
}

/// `downloads.v1.list`.
pub async fn list(data_dir: &Path) -> Result<ListDownloadsResult, BridgeError> {
    call(
        data_dir,
        methods::LIST,
        Value::Object(Default::default()),
        false,
    )
    .await
}

/// `downloads.v1.add` for one http(s) link, with the caller's idempotency key.
pub async fn add(
    data_dir: &Path,
    url: String,
    key: String,
) -> Result<DownloadSnapshot, BridgeError> {
    check_url(&url).map_err(BridgeError::invalid)?;
    if !is_idempotency_key(&key) {
        return Err(BridgeError::invalid(
            "the request key must be 8–128 letters, digits, '-' or '_'",
        ));
    }
    let params = AddDownloadParams {
        idempotency_key: key,
        source: DownloadSource { url },
        destination_profile_id: PROFILE.to_owned(),
        expected_size_bytes: None,
        expected_sha256: None,
    };
    let params = serde_json::to_value(&params)
        .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, methods::ADD, params, true).await
}

/// `downloads.v1.pause` / `resume` / `cancel` for one job.
pub async fn control(
    data_dir: &Path,
    control: Control,
    job_id: &str,
) -> Result<DownloadSnapshot, BridgeError> {
    let job_id = JobId::parse(job_id).map_err(|_| BridgeError::invalid("not a job ID"))?;
    let params = serde_json::to_value(JobRef { job_id })
        .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, control.method(), params, true).await
}

/// A fixed service method with typed params, for the shell's own lifecycle commands (never
/// reachable with a method name from the UI). Acknowledgement semantics are those of a mutation.
pub(crate) async fn call_value<P: Serialize, T: DeserializeOwned>(
    data_dir: &Path,
    method: &'static str,
    params: &P,
) -> Result<T, BridgeError> {
    let params = serde_json::to_value(params)
        .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, method, params, true).await
}

async fn call<T: DeserializeOwned>(
    data_dir: &Path,
    method: &str,
    params: Value,
    mutation: bool,
) -> Result<T, BridgeError> {
    // Nothing has been sent yet: any failure here is "unavailable", never "uncertain".
    let mut client = match tokio::time::timeout(CONNECT_TIMEOUT, Client::connect(data_dir)).await {
        Ok(Ok(client)) => client,
        Ok(Err(_)) => {
            return Err(BridgeError::unavailable(
                "the download service is not running",
            ));
        }
        Err(_) => {
            return Err(BridgeError::unavailable(
                "the download service did not answer",
            ));
        }
    };
    let limit = if mutation {
        MUTATION_TIMEOUT
    } else {
        LIST_TIMEOUT
    };
    let lost = |why: &str| {
        if mutation {
            BridgeError::uncertain(why)
        } else {
            BridgeError::unavailable(why)
        }
    };
    let result = match tokio::time::timeout(limit, client.call(method, params)).await {
        Err(_) => return Err(lost("no answer from the download service in time")),
        Ok(Err(CallError::Unavailable(_))) => {
            return Err(lost("the connection to the download service was lost"));
        }
        Ok(Err(CallError::Rpc(rpc))) => {
            return Err(BridgeError {
                outcome: Outcome::Rejected,
                error: rpc.data.unwrap_or_else(|| {
                    ErrorKind::Internal.to_error_data("the service refused the request")
                }),
            });
        }
        Ok(Ok(result)) => result,
    };
    serde_json::from_value(result).map_err(|_| lost("the download service sent a malformed answer"))
}

#[cfg(test)]
mod tests;
