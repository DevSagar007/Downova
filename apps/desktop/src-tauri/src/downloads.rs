//! Download commands (B2.1): the only path from the UI to `downloads.v1.*`, and since B2.8–B2.11
//! to `settings.v1.*` and `queues.v1.*`.
//!
//! Fixed operations, each validated here before anything is sent, forwarded over the private IPC
//! endpoint with the shared `dm-platform` client, and answered with the contract's own types.
//! There is no generic RPC forwarding: method names are constants, add always uses destination
//! profile `downloads` (the folder comes from the service's rules, never from the UI), and no file
//! content, database or engine detail crosses this boundary. Destination folders (Options) are
//! the only paths, checked here for shape and by the service for existence.
//!
//! Failures keep the service's structured `ErrorData` and say what the UI may assume:
//! - `unavailable`: nothing reached the service (no endpoint, connection refused or timed out);
//! - `uncertain`: a mutation was sent but no acknowledgement arrived — it may or may not have
//!   happened, so an add must be retried with the **same** idempotency key, never a new one;
//! - `rejected`: the service (or the argument checks here) refused it.

use std::path::Path;
use std::time::Duration;

use dm_domain::api::{
    AddDownloadParams, CheckLinkParams, CheckLinkResult, CreateQueueParams, DeleteQueueParams,
    DownloadSnapshot, DownloadSource, EmptyResult, HistoryCleanResult, HistoryPreview,
    HistoryPreviewParams, HistorySettings, JobRef, ListDownloadsResult, ListQueuesResult,
    LocateResult, MoveJobParams, MoveTarget, QueueInfo, QueueRef, RenameQueueParams, ReplaceMode,
    ReplaceSourceParams, ReplaceSourceResult, RetryParams, ServiceSettings, SetJobQueueParams,
    SetPriorityParams, SetQueueLimitParams, methods,
};
use dm_domain::policy::{DuplicatePolicy, FileCategory, Priority};
use dm_domain::validate::{check_url, is_idempotency_key};
use dm_domain::{AttemptId, ErrorData, ErrorKind, JobId, NextAction};
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
    pub(crate) fn unavailable_now(detail: &str) -> Self {
        Self::unavailable(detail)
    }

    pub(crate) fn rejected(error: ErrorData) -> Self {
        Self {
            outcome: Outcome::Rejected,
            error,
        }
    }

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
    /// Remove a finished job from the list (ADR 0013); never touches files.
    Remove,
}

impl Control {
    fn method(self) -> &'static str {
        match self {
            Control::Pause => methods::PAUSE,
            Control::Resume => methods::RESUME,
            Control::Cancel => methods::CANCEL,
            Control::Remove => methods::REMOVE,
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

/// Optional choices of the Add URL dialog (B2.8, B2.11), as the UI sends them.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub category: Option<String>,
    pub on_duplicate: Option<String>,
    pub queue_id: Option<String>,
    /// Trusted publisher SHA-256 (B2.18). Omitted when absent, so the request and its
    /// idempotency fingerprint stay exactly those of older clients.
    pub expected_sha256: Option<String>,
}

/// Exactly 64 hexadecimal digits, returned lower-case.
pub fn check_sha256(value: &str) -> Result<String, BridgeError> {
    let v = value.trim();
    if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(v.to_ascii_lowercase())
    } else {
        Err(BridgeError::invalid(
            "a SHA-256 checksum is exactly 64 hexadecimal digits",
        ))
    }
}

/// A queue ID as the service issues them (`main` or `q_` + lower-case hex).
pub fn is_queue_id(id: &str) -> bool {
    id == "main"
        || id.strip_prefix("q_").is_some_and(|hex| {
            !hex.is_empty()
                && hex.len() <= 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn queue_id(id: &str) -> Result<String, BridgeError> {
    if is_queue_id(id) {
        Ok(id.to_owned())
    } else {
        Err(BridgeError::invalid("not a queue ID"))
    }
}

fn job_id(id: &str) -> Result<JobId, BridgeError> {
    JobId::parse(id).map_err(|_| BridgeError::invalid("not a job ID"))
}

/// A closed enum value from the UI (`"high"`, `"refuse"`, `"top"` …); anything else is refused.
fn closed<T: serde::de::DeserializeOwned>(value: &str, what: &str) -> Result<T, BridgeError> {
    serde_json::from_value(Value::String(value.to_owned()))
        .map_err(|_| BridgeError::invalid(&format!("not a valid {what}")))
}

/// `downloads.v1.add` for one http(s) link, with the caller's idempotency key.
pub async fn add(
    data_dir: &Path,
    url: String,
    key: String,
    options: AddOptions,
) -> Result<DownloadSnapshot, BridgeError> {
    check_url(&url).map_err(BridgeError::invalid)?;
    if !is_idempotency_key(&key) {
        return Err(BridgeError::invalid(
            "the request key must be 8–128 letters, digits, '-' or '_'",
        ));
    }
    let category = options
        .category
        .as_deref()
        .map(|c| FileCategory::parse(c).ok_or_else(|| BridgeError::invalid("not a category")))
        .transpose()?;
    let on_duplicate = options
        .on_duplicate
        .as_deref()
        .map(|d| closed::<DuplicatePolicy>(d, "duplicate choice"))
        .transpose()?;
    let queue = options.queue_id.as_deref().map(queue_id).transpose()?;
    let expected_sha256 = options
        .expected_sha256
        .as_deref()
        .map(check_sha256)
        .transpose()?;
    let params = AddDownloadParams {
        idempotency_key: key,
        source: DownloadSource { url },
        destination_profile_id: PROFILE.to_owned(),
        expected_size_bytes: None,
        expected_sha256,
        category,
        on_duplicate,
        queue_id: queue,
    };
    let params = serde_json::to_value(&params)
        .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, methods::ADD, params, true).await
}

/// `downloads.v1.check`: unremoved jobs with the same link (a read; never "uncertain").
pub async fn check(data_dir: &Path, url: String) -> Result<CheckLinkResult, BridgeError> {
    check_url(&url).map_err(BridgeError::invalid)?;
    let params = serde_json::to_value(CheckLinkParams {
        source: DownloadSource { url },
    })
    .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, methods::CHECK, params, false).await
}

fn attempt_id(id: Option<String>) -> Result<Option<AttemptId>, BridgeError> {
    id.map(|a| AttemptId::parse(&a).map_err(|_| BridgeError::invalid("not an attempt ID")))
        .transpose()
}

fn operation_id(id: String) -> Result<String, BridgeError> {
    if is_idempotency_key(&id) {
        Ok(id)
    } else {
        Err(BridgeError::invalid(
            "the operation ID must be 8–128 letters, digits, '-' or '_'",
        ))
    }
}

/// `downloads.v1.retry` (B2.16). An uncertain answer is repeated with the **same** operation ID.
pub async fn retry(
    data_dir: &Path,
    id: &str,
    operation: String,
    expected_attempt_id: Option<String>,
    discard_partial: bool,
) -> Result<DownloadSnapshot, BridgeError> {
    let params = RetryParams {
        job_id: job_id(id)?,
        operation_id: operation_id(operation)?,
        expected_attempt_id: attempt_id(expected_attempt_id)?,
        discard_partial,
    };
    call_value(data_dir, methods::RETRY, &params).await
}

/// `downloads.v1.replace_source` (B2.17). The URL is checked for shape here and never echoed.
pub async fn replace_source(
    data_dir: &Path,
    id: &str,
    operation: String,
    url: String,
    mode: &str,
    expected_attempt_id: Option<String>,
) -> Result<ReplaceSourceResult, BridgeError> {
    check_url(&url).map_err(BridgeError::invalid)?;
    let params = ReplaceSourceParams {
        job_id: job_id(id)?,
        operation_id: operation_id(operation)?,
        url,
        mode: closed::<ReplaceMode>(mode, "replacement mode")?,
        expected_attempt_id: attempt_id(expected_attempt_id)?,
    };
    call_value(data_dir, methods::REPLACE_SOURCE, &params).await
}

/// `downloads.v1.locate` (B2.19): a read, used only by [`crate::folders::reveal`].
pub async fn locate(data_dir: &Path, id: &str) -> Result<LocateResult, BridgeError> {
    let params = serde_json::to_value(JobRef {
        job_id: job_id(id)?,
    })
    .map_err(|_| BridgeError::invalid("the request could not be encoded"))?;
    call(data_dir, methods::LOCATE, params, false).await
}

/// `downloads.v1.confirm_name`.
pub async fn confirm_name(data_dir: &Path, id: &str) -> Result<DownloadSnapshot, BridgeError> {
    let job_id = job_id(id)?;
    call_value(data_dir, methods::CONFIRM_NAME, &JobRef { job_id }).await
}

/// `downloads.v1.set_queue`.
pub async fn set_queue(
    data_dir: &Path,
    id: &str,
    queue: &str,
) -> Result<DownloadSnapshot, BridgeError> {
    let params = SetJobQueueParams {
        job_id: job_id(id)?,
        queue_id: queue_id(queue)?,
    };
    call_value(data_dir, methods::SET_QUEUE, &params).await
}

/// `downloads.v1.set_priority`.
pub async fn set_priority(
    data_dir: &Path,
    id: &str,
    priority: &str,
) -> Result<DownloadSnapshot, BridgeError> {
    let params = SetPriorityParams {
        job_id: job_id(id)?,
        priority: closed::<Priority>(priority, "priority")?,
    };
    call_value(data_dir, methods::SET_PRIORITY, &params).await
}

/// `downloads.v1.move` (manual execution order).
pub async fn move_job(
    data_dir: &Path,
    id: &str,
    to: &str,
) -> Result<DownloadSnapshot, BridgeError> {
    let params = MoveJobParams {
        job_id: job_id(id)?,
        to: closed::<MoveTarget>(to, "position")?,
    };
    call_value(data_dir, methods::MOVE, &params).await
}

/// Longest destination folder the shell forwards (the service checks the folder itself).
pub const MAX_DIRECTORY: usize = 1024;

/// `settings.v1.get`.
pub async fn settings(data_dir: &Path) -> Result<ServiceSettings, BridgeError> {
    call(
        data_dir,
        methods::SETTINGS_GET,
        Value::Object(Default::default()),
        false,
    )
    .await
}

/// `settings.v1.set`: categories and the collision policy are closed enums (typed); each folder
/// must be an absolute path without `.`/`..` parts or control characters. Whether it exists is
/// the service's check.
pub async fn set_settings(
    data_dir: &Path,
    settings: ServiceSettings,
) -> Result<ServiceSettings, BridgeError> {
    for rule in &settings.destination_rules {
        check_directory(&rule.directory)?;
    }
    call_value(data_dir, methods::SETTINGS_SET, &settings).await
}

/// `history.v1.get` (B2.12).
pub async fn history(data_dir: &Path) -> Result<HistorySettings, BridgeError> {
    call(
        data_dir,
        methods::HISTORY_GET,
        Value::Object(Default::default()),
        false,
    )
    .await
}

/// `history.v1.set`: 1–3650 days or keep forever (`None`); the service checks again.
pub async fn set_history(
    data_dir: &Path,
    retention_days: Option<u32>,
) -> Result<HistorySettings, BridgeError> {
    check_retention(retention_days)?;
    call_value(
        data_dir,
        methods::HISTORY_SET,
        &HistorySettings { retention_days },
    )
    .await
}

/// `history.v1.preview`: changes nothing.
pub async fn preview_history(
    data_dir: &Path,
    retention_days: Option<u32>,
) -> Result<HistoryPreview, BridgeError> {
    check_retention(retention_days)?;
    call_value(
        data_dir,
        methods::HISTORY_PREVIEW,
        &HistoryPreviewParams { retention_days },
    )
    .await
}

/// `history.v1.clean`: removes due list entries now; never files.
pub async fn clean_history(data_dir: &Path) -> Result<HistoryCleanResult, BridgeError> {
    call_value(data_dir, methods::HISTORY_CLEAN, &EmptyResult {}).await
}

fn check_retention(days: Option<u32>) -> Result<(), BridgeError> {
    match days {
        Some(d) if !(1..=3650).contains(&d) => Err(BridgeError::invalid(
            "history retention must be 1 to 3650 days, or keep forever",
        )),
        _ => Ok(()),
    }
}

/// Shape check of a destination folder from the UI.
pub fn check_directory(dir: &str) -> Result<(), BridgeError> {
    use std::path::Component;
    let path = Path::new(dir);
    if dir.is_empty()
        || dir.len() > MAX_DIRECTORY
        || dir.chars().any(char::is_control)
        || !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(BridgeError::invalid(
            "a download folder must be an absolute path without . or .. parts",
        ));
    }
    Ok(())
}

/// `queues.v1.list`.
pub async fn queues(data_dir: &Path) -> Result<ListQueuesResult, BridgeError> {
    call(
        data_dir,
        methods::QUEUES_LIST,
        Value::Object(Default::default()),
        false,
    )
    .await
}

fn queue_name(name: &str) -> Result<String, BridgeError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(BridgeError::invalid(
            "a queue name must be 1–64 printable characters",
        ));
    }
    Ok(name.to_owned())
}

fn queue_limit(n: u32) -> Result<u32, BridgeError> {
    if (1..=16).contains(&n) {
        Ok(n)
    } else {
        Err(BridgeError::invalid("a queue runs 1–16 downloads at once"))
    }
}

/// `queues.v1.create`.
pub async fn create_queue(
    data_dir: &Path,
    name: &str,
    max_active: Option<u32>,
) -> Result<QueueInfo, BridgeError> {
    let params = CreateQueueParams {
        name: queue_name(name)?,
        max_active: max_active.map(queue_limit).transpose()?,
    };
    call_value(data_dir, methods::QUEUES_CREATE, &params).await
}

/// `queues.v1.rename`.
pub async fn rename_queue(data_dir: &Path, id: &str, name: &str) -> Result<QueueInfo, BridgeError> {
    let params = RenameQueueParams {
        queue_id: queue_id(id)?,
        name: queue_name(name)?,
    };
    call_value(data_dir, methods::QUEUES_RENAME, &params).await
}

/// `queues.v1.set_limit`.
pub async fn set_queue_limit(
    data_dir: &Path,
    id: &str,
    max_active: u32,
) -> Result<QueueInfo, BridgeError> {
    let params = SetQueueLimitParams {
        queue_id: queue_id(id)?,
        max_active: queue_limit(max_active)?,
    };
    call_value(data_dir, methods::QUEUES_SET_LIMIT, &params).await
}

/// `queues.v1.delete` (never deletes jobs or files; see the service).
pub async fn delete_queue(
    data_dir: &Path,
    id: &str,
    move_to: Option<&str>,
) -> Result<EmptyResult, BridgeError> {
    let params = DeleteQueueParams {
        queue_id: queue_id(id)?,
        move_to: move_to.map(queue_id).transpose()?,
    };
    call_value(data_dir, methods::QUEUES_DELETE, &params).await
}

/// `queues.v1.start` / `queues.v1.stop`.
pub async fn set_queue_running(
    data_dir: &Path,
    id: &str,
    running: bool,
) -> Result<QueueInfo, BridgeError> {
    let params = QueueRef {
        queue_id: queue_id(id)?,
    };
    let method = if running {
        methods::QUEUES_START
    } else {
        methods::QUEUES_STOP
    };
    call_value(data_dir, method, &params).await
}

/// `downloads.v1.pause` / `resume` / `cancel` / `remove` for one job.
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
