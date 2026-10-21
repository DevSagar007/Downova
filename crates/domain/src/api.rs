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
