//! Service API v1 wire types.
//!
//! Mirrors `packages/contracts/schema/service-api.v1.schema.json` (the canonical source).
//! Field names, optionality and enums must match the schema exactly; `tests/contract_examples.rs`
//! deserializes every contract example to enforce this.

use serde::{Deserialize, Serialize};

use crate::policy::{CollisionPolicy, DuplicatePolicy, FileCategory, Priority};
use crate::{
    AttemptId, Blocker, ByteCount, CapabilityReport, DesiredState, ErrorData, JobId, Phase,
};

/// Version of the service API described by this module.
pub const API_VERSION: u32 = 1;

/// Backend identity and capability report returned by `service.v1.hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendInfo {
    pub id: String,
    pub version: Option<String>,
    pub capabilities: Vec<CapabilityReport>,
}

/// Result of `service.v1.hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloResult {
    pub api_version: u32,
    pub service_version: String,
    pub service_instance_id: String,
    pub backend: BackendInfo,
}

/// Download source. Contains the raw URL, which may embed secrets: never log it directly,
/// use [`crate::redact::redact_url`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadSource {
    pub url: String,
}

/// Params of `downloads.v1.add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddDownloadParams {
    pub idempotency_key: String,
    pub source: DownloadSource,
    pub destination_profile_id: String,
    #[serde(default)]
    pub expected_size_bytes: Option<ByteCount>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    /// Destination category (B2.8, ADR 0015). Omitted: chosen from the link's file name. The
    /// folder comes from the service's destination rules, never from the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<FileCategory>,
    /// Omitted means `allow` (the behaviour before B2.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_duplicate: Option<DuplicatePolicy>,
    /// Queue to add the job to (B2.11, ADR 0017). Omitted: the default queue `main`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_id: Option<String>,
}

/// Sanitized job state for UI/CLI. Progress is live state, not a durability checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadSnapshot {
    pub job_id: JobId,
    pub attempt_id: Option<AttemptId>,
    pub display_name: String,
    pub source_display: String,
    pub desired_state: DesiredState,
    pub phase: Phase,
    pub blockers: Vec<Blocker>,
    pub downloaded_bytes: ByteCount,
    pub total_bytes: Option<ByteCount>,
    pub error: Option<ErrorData>,
    /// Set once the job completed (B1.3): SHA-256 of the final file and how it was checked.
    /// Optional in the contract: omitted (not `null`) when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    /// Removed from the list (B2.3, ADR 0013): omitted from `downloads.v1.list`, still answered by
    /// `get` and by replays of the original add. Omitted when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    /// Queue, priority and manual position (B2.11). Optional in the contract for older services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<ByteCount>,
    /// Destination category the job was filed under (B2.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<FileCategory>,
}

/// How a completed file was checked (B1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    /// Lower-case hex SHA-256 of the final file, computed by Downova.
    pub sha256: String,
    pub source: VerificationSource,
}

/// Whether a computed hash was compared with a trusted checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSource {
    /// Computed locally; there was no trusted checksum to compare with.
    LocalHash,
    /// Matched the trusted `expected_sha256` given with the request.
    PublisherChecksum,
}

/// Kind of a `events.v1.subscribe` stream item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SnapshotRequired,
    JobAdded,
    JobChanged,
    Progress,
    JobRemoved,
}

/// Event stream item. A sequence gap or a new `service_instance_id` requires a fresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceEvent {
    pub service_instance_id: String,
    pub sequence: ByteCount,
    pub kind: EventKind,
    pub job_id: Option<JobId>,
    pub attempt_id: Option<AttemptId>,
    pub snapshot: Option<DownloadSnapshot>,
}

/// Params of `downloads.v1.check` (B2.8): is this link already in the list?
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckLinkParams {
    pub source: DownloadSource,
}

/// One existing job with the same link. Never carries the URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateSummary {
    pub job_id: JobId,
    pub display_name: String,
    pub phase: Phase,
    pub desired_state: DesiredState,
}

/// Result of `downloads.v1.check`: unremoved jobs with the same link identity, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckLinkResult {
    pub duplicates: Vec<DuplicateSummary>,
}

/// A category to folder mapping (B2.8). The folder is an absolute local directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationRule {
    pub category: FileCategory,
    pub directory: String,
}

/// Service settings (`settings.v1.get` result and `settings.v1.set` params). `set` replaces
/// the whole value; a category without a rule uses the default download folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSettings {
    pub destination_rules: Vec<DestinationRule>,
    pub on_collision: CollisionPolicy,
}

/// `history.v1.get` result and `history.v1.set` params (B2.12, ADR 0018). `retention_days`:
/// finished jobs (completed, failed, cancelled) leave the list this many days after they
/// finished; `null` keeps history indefinitely (the default). Files are never touched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySettings {
    #[serde(deserialize_with = "required_days")]
    pub retention_days: Option<u32>,
}

/// `retention_days` must be present (`null` means keep forever): a missing field is an error, so
/// a malformed request can never silently mean "keep forever".
fn required_days<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(d)
}

/// `history.v1.preview` params: the policy to evaluate (not saved).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPreviewParams {
    #[serde(deserialize_with = "required_days")]
    pub retention_days: Option<u32>,
}

/// `history.v1.preview` result: how many listed finished jobs the policy would remove now, of
/// how many listed finished jobs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPreview {
    pub expiring: u32,
    pub finished: u32,
}

/// `history.v1.clean` result: jobs removed from the list now, and eligible jobs that stayed
/// because their removal could not be recorded (retried by the next sweep).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCleanResult {
    pub removed: u32,
    pub kept: u32,
}

/// A download queue (B2.11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueInfo {
    pub queue_id: String,
    pub name: String,
    /// Order of the queue in the list; earlier queues win priority ties.
    pub position: u32,
    /// Files of this queue transferring at once (1 to 16), within the global limit.
    pub max_active: u32,
    /// False after Stop queue: its jobs hold with `queue_stopped`.
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQueuesResult {
    pub queues: Vec<QueueInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateQueueParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueRef {
    pub queue_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameQueueParams {
    pub queue_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetQueueLimitParams {
    pub queue_id: String,
    pub max_active: u32,
}

/// Deleting a queue never deletes jobs or files: a queue with jobs is refused unless `move_to`
/// names another queue that receives them. The default queue cannot be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteQueueParams {
    pub queue_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetJobQueueParams {
    pub job_id: JobId,
    pub queue_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPriorityParams {
    pub job_id: JobId,
    pub priority: Priority,
}

/// Manual reordering inside a queue (execution order, not a display sort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveTarget {
    Up,
    Down,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveJobParams {
    pub job_id: JobId,
    pub to: MoveTarget,
}

/// Result of `events.v1.subscribe` (B2.9): an authoritative list and the sequence it reflects.
/// Every later `events.v1.event` notification on the same connection has `sequence` equal to
/// the previous one plus 1; a gap or another `service_instance_id` means: subscribe again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeResult {
    pub service_instance_id: String,
    pub sequence: ByteCount,
    pub items: Vec<DownloadSnapshot>,
}

/// A notification sent by the service on a subscribed connection (no `id`, no answer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: ServiceEvent,
}

/// Params of `downloads.v1.get` / `pause` / `resume` / `cancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRef {
    pub job_id: JobId,
}

/// Result of `downloads.v1.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListDownloadsResult {
    pub items: Vec<DownloadSnapshot>,
}

/// Result of acknowledge-only methods.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyResult {}

/// How `service.v1.shutdown` leaves unfinished downloads (ADR 0012).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownMode {
    /// They continue after the next start (Ctrl+C, `{}`).
    #[default]
    KeepState,
    /// "Exit and stop downloads": each running download is paused as if by the user, durably,
    /// before the acknowledgement.
    PauseDownloads,
}

/// Params of `service.v1.shutdown`. `{}` means [`ShutdownMode::KeepState`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ShutdownMode>,
}

impl ShutdownParams {
    /// The requested mode; absent means [`ShutdownMode::KeepState`].
    pub fn mode(&self) -> ShutdownMode {
        self.mode.unwrap_or_default()
    }
}

/// Acknowledgement of `service.v1.shutdown`: the paused intent is durable; the process has not
/// necessarily exited yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownResult {
    pub mode: ShutdownMode,
    pub paused_jobs: u32,
}

/// Method names of service API v1 (closed set; no engine passthrough).
pub mod methods {
    pub const HELLO: &str = "service.v1.hello";
    pub const SHUTDOWN: &str = "service.v1.shutdown";
    pub const ADD: &str = "downloads.v1.add";
    pub const LIST: &str = "downloads.v1.list";
    pub const GET: &str = "downloads.v1.get";
    pub const PAUSE: &str = "downloads.v1.pause";
    pub const RESUME: &str = "downloads.v1.resume";
    pub const CANCEL: &str = "downloads.v1.cancel";
    pub const REMOVE: &str = "downloads.v1.remove";
    pub const CHECK: &str = "downloads.v1.check";
    pub const CONFIRM_NAME: &str = "downloads.v1.confirm_name";
    pub const SET_QUEUE: &str = "downloads.v1.set_queue";
    pub const SET_PRIORITY: &str = "downloads.v1.set_priority";
    pub const MOVE: &str = "downloads.v1.move";
    pub const SETTINGS_GET: &str = "settings.v1.get";
    pub const SETTINGS_SET: &str = "settings.v1.set";
    pub const QUEUES_LIST: &str = "queues.v1.list";
    pub const QUEUES_CREATE: &str = "queues.v1.create";
    pub const QUEUES_RENAME: &str = "queues.v1.rename";
    pub const QUEUES_DELETE: &str = "queues.v1.delete";
    pub const QUEUES_SET_LIMIT: &str = "queues.v1.set_limit";
    pub const QUEUES_START: &str = "queues.v1.start";
    pub const QUEUES_STOP: &str = "queues.v1.stop";
    pub const SUBSCRIBE: &str = "events.v1.subscribe";
    /// Notification method of subscribed connections (never a request).
    pub const EVENT: &str = "events.v1.event";
    pub const HISTORY_GET: &str = "history.v1.get";
    pub const HISTORY_SET: &str = "history.v1.set";
    pub const HISTORY_PREVIEW: &str = "history.v1.preview";
    pub const HISTORY_CLEAN: &str = "history.v1.clean";
    pub const ALL: [&str; 28] = [
        HELLO,
        SHUTDOWN,
        ADD,
        LIST,
        GET,
        PAUSE,
        RESUME,
        CANCEL,
        REMOVE,
        CHECK,
        CONFIRM_NAME,
        SET_QUEUE,
        SET_PRIORITY,
        MOVE,
        SETTINGS_GET,
        SETTINGS_SET,
        QUEUES_LIST,
        QUEUES_CREATE,
        QUEUES_RENAME,
        QUEUES_DELETE,
        QUEUES_SET_LIMIT,
        QUEUES_START,
        QUEUES_STOP,
        SUBSCRIBE,
        HISTORY_GET,
        HISTORY_SET,
        HISTORY_PREVIEW,
        HISTORY_CLEAN,
    ];
}

/// JSON-RPC 2.0 request (one per line on the IPC endpoint).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// JSON-RPC error codes used by the service.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const SERVICE_ERROR: i64 = -32000;
}

/// JSON-RPC error object; `data` carries the stable [`ErrorData`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<ErrorData>,
}

impl RpcError {
    /// Builds an error whose message is the stable kind name.
    pub fn new(code: i64, data: ErrorData) -> Self {
        Self {
            code,
            message: data.kind.wire_name(),
            data: Some(data),
        }
    }
}

/// JSON-RPC 2.0 response. Exactly one of `result` / `error` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn ok(id: Option<String>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<String>, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Browser native-host request (B0.6). A closed set: there is no arbitrary RPC forwarding.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeHostRequest {
    /// Is a compatible download service reachable?
    Hello { v: u32, request_id: String },
    /// Explicit hand-off of one public http(s) GET link.
    Add {
        v: u32,
        request_id: String,
        /// Raw URL (may contain signed-query tokens). Never printed; see the `Debug` impl.
        url: String,
        idempotency_key: String,
    },
}

impl NativeHostRequest {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Hello { request_id, .. } | Self::Add { request_id, .. } => request_id,
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            Self::Hello { v, .. } | Self::Add { v, .. } => *v,
        }
    }
}

impl std::fmt::Debug for NativeHostRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hello { v, request_id } => f
                .debug_struct("Hello")
                .field("v", v)
                .field("request_id", request_id)
                .finish(),
            Self::Add {
                v,
                request_id,
                url,
                idempotency_key,
            } => f
                .debug_struct("Add")
                .field("v", v)
                .field("request_id", request_id)
                .field("url", &crate::redact::redact_url(url))
                .field("idempotency_key", idempotency_key)
                .finish(),
        }
    }
}

/// Outcome reported by the native host. `Accepted` means the service created (or, for the same
/// idempotency key, returned) the job; it does not mean a transfer started or finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeHostOutcome {
    ServiceReady,
    Accepted,
    Rejected,
    Unavailable,
    /// No acknowledgement in time: retry with the same idempotency key.
    Uncertain,
}

/// Browser native-host response (B0.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHostResponse {
    pub v: u32,
    /// `None` only when the request could not be parsed.
    pub request_id: Option<String>,
    pub outcome: NativeHostOutcome,
    #[serde(default)]
    pub job: Option<DownloadSnapshot>,
    #[serde(default)]
    pub hello: Option<HelloResult>,
    #[serde(default)]
    pub error: Option<ErrorData>,
}
