//! Service API v1 wire types.
//!
//! Mirrors `packages/contracts/schema/service-api.v1.schema.json` (the canonical source).
//! Field names, optionality and enums must match the schema exactly; `tests/contract_examples.rs`
//! deserializes every contract example to enforce this.

use serde::{Deserialize, Serialize};

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

/// Result of acknowledge-only methods such as `service.v1.shutdown`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyResult {}

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
    pub const ALL: [&str; 8] = [HELLO, SHUTDOWN, ADD, LIST, GET, PAUSE, RESUME, CANCEL];
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
