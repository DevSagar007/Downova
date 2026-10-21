/* eslint-disable */
// GENERATED FILE — DO NOT EDIT.
// Source: packages/contracts/schema/service-api.v1.schema.json
// Regenerate with: pnpm contracts:gen

/**
 * Service-owned logical job identifier. Never an aria2 GID.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "JobId".
 */
export type JobId = string;
/**
 * Identifier of one transfer attempt of a job. Events from stale attempts are rejected.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "AttemptId".
 */
export type AttemptId = string;
/**
 * Unsigned 64-bit byte count encoded as a decimal string (no sign, no leading zeros).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ByteCount".
 */
export type ByteCount = string;
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Sha256Hex".
 */
export type Sha256Hex = string;
/**
 * What the user asked for. Only user commands change it; policy never does.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DesiredState".
 */
export type DesiredState = "running" | "paused" | "cancelled";
/**
 * Where the job is in its lifecycle, independent of desired state and blockers.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Phase".
 */
export type Phase =
  | "queued"
  | "probing"
  | "transferring"
  | "verifying"
  | "scanning"
  | "finalizing"
  | "completed"
  | "failed"
  | "cancelled";
/**
 * Automatic policy condition holding an eligible job. Distinct from a manual user pause.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Blocker".
 */
export type Blocker =
  | "offline"
  | "metered_network"
  | "schedule_window"
  | "rate_limit_cooldown"
  | "disk_unavailable"
  | "volume_missing"
  | "queue_limit";
/**
 * Stable, user-explainable failure classification (research §17.6).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ErrorKind".
 */
export type ErrorKind =
  | "NETWORK_OFFLINE"
  | "AUTH_REQUIRED"
  | "LINK_REFRESH_REQUIRED"
  | "REMOTE_CHANGED"
  | "RANGE_UNSUPPORTED"
  | "INVALID_RANGE"
  | "CHECKSUM_MISMATCH"
  | "DISK_FULL"
  | "VOLUME_MISSING"
  | "HOST_NOT_INSTALLED"
  | "SCAN_BLOCKED"
  | "BACKEND_CAPABILITY_UNAVAILABLE"
  | "SERVICE_UNAVAILABLE"
  | "INVALID_REQUEST"
  | "IDEMPOTENCY_CONFLICT"
  | "INTERNAL";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NextAction".
 */
export type NextAction =
  | "none"
  | "retry"
  | "wait"
  | "refresh_link"
  | "sign_in"
  | "free_disk_space"
  | "reconnect_volume"
  | "install_desktop_app"
  | "choose_restart_or_keep"
  | "contact_support";
/**
 * Product-level capability a backend may or may not provide.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Capability".
 */
export type Capability =
  | "http"
  | "https"
  | "segmented_http"
  | "resume_strong_validator"
  | "speed_limit_global"
  | "speed_limit_per_job"
  | "http_proxy"
  | "socks5_proxy"
  | "pac_proxy"
  | "http2"
  | "ftp"
  | "sftp"
  | "bittorrent"
  | "metalink";
/**
 * unverified = documented upstream but not yet proven by project fixtures.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CapabilitySupport".
 */
export type CapabilitySupport = "supported" | "unsupported" | "unverified";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "EventKind".
 */
export type EventKind = "snapshot_required" | "job_added" | "job_changed" | "progress" | "job_removed";
export type RpcResponse =
  | {
      result: unknown;
    }
  | {
      error: unknown;
    };

/**
 * Canonical service API contract v1. Source of truth for Rust (crates/domain::api) and generated TypeScript (packages/contracts/src/generated). See docs/architecture.md#service-api.
 */
export interface ServiceApiV1 {}
/**
 * Carried in JSON-RPC error.data and in failed job snapshots. Never contains secrets.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ErrorData".
 */
export interface ErrorData {
  kind: ErrorKind;
  retryable: boolean;
  next_action: NextAction;
  /**
   * Sanitized, user-safe detail. Must pass redaction rules.
   */
  detail?: string | null;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CapabilityReport".
 */
export interface CapabilityReport {
  capability: Capability;
  support: CapabilitySupport;
  note?: string | null;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "BackendInfo".
 */
export interface BackendInfo {
  id: string;
  version: string | null;
  capabilities: CapabilityReport[];
}
/**
 * Result of service.v1.hello.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "HelloResult".
 */
export interface HelloResult {
  api_version: 1;
  service_version: string;
  service_instance_id: string;
  backend: BackendInfo;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DownloadSource".
 */
export interface DownloadSource {
  /**
   * http(s) URL. Never logged unredacted.
   */
  url: string;
}
/**
 * Params of downloads.v1.add. Same key + same request => same job; same key + different payload => IDEMPOTENCY_CONFLICT.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "AddDownloadParams".
 */
export interface AddDownloadParams {
  idempotency_key: string;
  source: DownloadSource;
  destination_profile_id: string;
  expected_size_bytes?: ByteCount | null;
  expected_sha256?: Sha256Hex | null;
}
/**
 * Sanitized job state presented by UI/CLI. Progress is live, not a durability checkpoint.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DownloadSnapshot".
 */
export interface DownloadSnapshot {
  job_id: JobId;
  attempt_id: AttemptId | null;
  display_name: string;
  /**
   * Redacted URL: no userinfo, query values or fragment.
   */
  source_display: string;
  desired_state: DesiredState;
  phase: Phase;
  blockers: Blocker[];
  downloaded_bytes: ByteCount;
  total_bytes: ByteCount | null;
  error: ErrorData | null;
}
/**
 * events.v1.subscribe stream item. A sequence gap or new service_instance_id requires a fresh snapshot.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ServiceEvent".
 */
export interface ServiceEvent {
  service_instance_id: string;
  sequence: ByteCount;
  kind: EventKind;
  job_id: JobId | null;
  attempt_id: AttemptId | null;
  snapshot: DownloadSnapshot | null;
}
/**
 * Params of downloads.v1.get / pause / resume / cancel.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "JobRef".
 */
export interface JobRef {
  job_id: JobId;
}
/**
 * Result of downloads.v1.list (sanitized snapshots, insertion order).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ListDownloadsResult".
 */
export interface ListDownloadsResult {
  items: DownloadSnapshot[];
}
/**
 * Result of methods that only acknowledge (e.g. service.v1.shutdown).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "EmptyResult".
 */
export interface EmptyResult {}
/**
 * JSON-RPC 2.0 request, one per line over the local IPC endpoint. Request id correlation is separate from job idempotency.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RpcRequest".
 */
export interface RpcRequest {
  jsonrpc: "2.0";
  id: string;
  method:
    | "service.v1.hello"
    | "service.v1.shutdown"
    | "downloads.v1.add"
    | "downloads.v1.list"
    | "downloads.v1.get"
    | "downloads.v1.pause"
    | "downloads.v1.resume"
    | "downloads.v1.cancel";
  params: {};
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RpcError".
 */
export interface RpcError {
  /**
   * -32700 parse, -32600 invalid request, -32601 unknown method, -32602 invalid params, -32000 download/service error.
   */
  code: number;
  message: string;
  data: ErrorData | null;
}
export interface RpcResponse1 {
  jsonrpc: "2.0";
  id: string | null;
  result?: unknown;
  error?: RpcError;
}
