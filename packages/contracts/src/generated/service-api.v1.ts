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
 * Automatic policy condition holding an eligible job. Distinct from a manual user pause. sign_in_required (ADR 0028): a signed-in download waits, keeping its partial data, until it is reconnected from the browser or the credential store is unlocked.
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
  | "queue_limit"
  | "queue_stopped"
  | "name_conflict"
  | "sign_in_required";
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
  | "DESTINATION_UNWRITABLE"
  | "HOST_NOT_INSTALLED"
  | "SCAN_BLOCKED"
  | "BACKEND_CAPABILITY_UNAVAILABLE"
  | "SERVER_BUSY"
  | "TLS_HANDSHAKE_FAILED"
  | "SERVICE_UNAVAILABLE"
  | "INVALID_REQUEST"
  | "IDEMPOTENCY_CONFLICT"
  | "DUPLICATE_DOWNLOAD"
  | "AUTH_SCOPE_UNSUPPORTED"
  | "CREDENTIAL_STORE_UNAVAILABLE"
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
  | "durable_resume"
  | "speed_limit_global"
  | "speed_limit_per_job"
  | "http_proxy"
  | "socks5_proxy"
  | "pac_proxy"
  | "http2"
  | "ftp"
  | "sftp"
  | "bittorrent"
  | "metalink"
  | "authenticated_get";
/**
 * unverified = documented upstream but not yet proven by project fixtures.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CapabilitySupport".
 */
export type CapabilitySupport = "supported" | "unsupported" | "unverified";
/**
 * What downloads.v1.add does when an unremoved job has the same link identity. Omitted means 'allow' (the behaviour before B2.8).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DuplicatePolicy".
 */
export type DuplicatePolicy = "allow" | "refuse";
/**
 * A bandwidth limit in bytes per second (CORE-06, ADR 0024): a canonical decimal string from 1024 to 1000000000000.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SpeedLimitValue".
 */
export type SpeedLimitValue = string;
/**
 * Queue identifier: 'main' (the default queue) or a service-generated 'q_…'.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "QueueId".
 */
export type QueueId = string;
/**
 * Admission priority (B2.11, ADR 0017). Global: a higher priority starts first; ties follow queue order, then manual position, then acceptance time.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Priority".
 */
export type Priority = "low" | "normal" | "high";
/**
 * File-type category for destination rules (B2.8, ADR 0015). 'other' is everything no default extension rule matches.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "FileCategory".
 */
export type FileCategory = "compressed" | "documents" | "music" | "programs" | "video" | "other";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "BrowserKind".
 */
export type BrowserKind = "chrome" | "edge";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CredentialMode".
 */
export type CredentialMode = "temporary" | "remembered";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CredentialState".
 */
export type CredentialState = "available" | "missing" | "locked" | "unavailable" | "expired" | "forgotten";
/**
 * no_resumable_checkpoint: the engine has confirmed no resumable checkpoint for this transfer (the server may not support resuming, or none is saved yet), so holding it would discard its progress; it is held as soon as a checkpoint exists.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ScheduleExemption".
 */
export type ScheduleExemption = "no_resumable_checkpoint";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "EventKind".
 */
export type EventKind = "snapshot_required" | "job_added" | "job_changed" | "progress" | "job_removed";
/**
 * How service.v1.shutdown leaves unfinished downloads (ADR 0012). keep_state: they continue after the next start (the default, also used for Ctrl+C). pause_downloads: "Exit and stop downloads" — each running download is paused (manual pause, persisted with its checkpoint) and stays paused until the user resumes it.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ShutdownMode".
 */
export type ShutdownMode = "keep_state" | "pause_downloads";
export type RpcResponse =
  | {
      result: unknown;
    }
  | {
      error: unknown;
    };
/**
 * One native-messaging request from the extension. Only these operations exist; arbitrary service RPC is never forwarded.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostRequest".
 */
export type NativeHostRequest =
  NativeHostHello | NativeHostAdd | NativeHostAddAuthenticated | NativeHostReconnectCandidates | NativeHostReconnect;
/**
 * service_ready: hello answered. accepted: the service created (or returned, for the same key) the job — not that a transfer started or finished. rejected: the request or URL is not supported. unavailable: host, service or capability missing. uncertain: no acknowledgement in time; retry with the SAME idempotency key.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostOutcome".
 */
export type NativeHostOutcome = "service_ready" | "accepted" | "rejected" | "unavailable" | "uncertain";
/**
 * When a final name is taken: 'rename' uses the next free name; 'ask' holds the verified file (blocker name_conflict) until downloads.v1.confirm_name or cancel. Nothing is ever overwritten.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CollisionPolicy".
 */
export type CollisionPolicy = "rename" | "ask";
/**
 * A bandwidth limit in bytes per second (CORE-06, ADR 0024): a ByteCount from 1024 to 1000000000000, or null for no limit. 0 is refused: it is neither 'unlimited' nor a pause.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SpeedLimit".
 */
export type SpeedLimit = SpeedLimitValue | null;
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Weekday".
 */
export type Weekday = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "MoveTarget".
 */
export type MoveTarget = "up" | "down" | "top" | "bottom";
/**
 * keep_if_verified keeps progress only when the replacement is proven to be the same content (otherwise nothing changes and the outcome is choice_required); restart uses the replacement from zero.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReplaceMode".
 */
export type ReplaceMode = "keep_if_verified" | "restart";
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReplaceOutcome".
 */
export type ReplaceOutcome = "progress_kept" | "restarted" | "choice_required";
/**
 * Why progress could not be kept: no_checkpoint (nothing resumable, e.g. a failed job), validator_missing, validator_changed, length_changed, range_ignored, content_differs (the confirmed bytes differ), different_resource (another origin or path without a trusted checksum), unverifiable (network, redirect or server error).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReplaceReason".
 */
export type ReplaceReason =
  | "no_checkpoint"
  | "validator_missing"
  | "validator_changed"
  | "length_changed"
  | "range_ignored"
  | "content_differs"
  | "different_resource"
  | "unverifiable";
/**
 * Language of the notifications the service shows (CORE-09).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NotificationLocale".
 */
export type NotificationLocale = "en" | "bn";
/**
 * available: this service has a system notification adapter (whether the system displays a notification is not observable); unavailable: none.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NotificationAvailability".
 */
export type NotificationAvailability = "available" | "unavailable";

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
  /**
   * The cause is transient: the same request may succeed later unchanged. Not a promise of an automatic retry.
   */
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
  /**
   * Destination category; omitted: chosen from the link's file name. The folder comes from the service's destination rules, never from the client.
   */
  category?: "compressed" | "documents" | "music" | "programs" | "video" | "other";
  on_duplicate?: DuplicatePolicy;
  /**
   * Queue for the job; omitted: 'main'.
   */
  queue_id?: string;
  /**
   * This download's own bandwidth limit (CORE-06, ADR 0024). Omitted or null: no limit of its own; only the global limit applies.
   */
  speed_limit_bytes_per_second?: SpeedLimitValue | null;
}
/**
 * How a completed file was checked (B1.3). local_hash: Downova computed the SHA-256 of the placed file; nothing trusted was given to compare it with. publisher_checksum: the computed SHA-256 matched the trusted expected_sha256 given with the request.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "Verification".
 */
export interface Verification {
  sha256: string;
  source: "local_hash" | "publisher_checksum";
}
/**
 * Sanitized job state presented by UI/CLI. Progress is live, not a durability checkpoint.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DownloadSnapshot".
 */
export interface DownloadSnapshot {
  /**
   * Set when the job completed: the SHA-256 of the final file and whether it matched a trusted checksum. Absent or null otherwise.
   */
  verification?: Verification | null;
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
  /**
   * True once the job was removed from the list (ADR 0013): it is omitted from downloads.v1.list but still answers get and replays of its original add. Absent or false otherwise. Removal never deletes files.
   */
  removed?: boolean;
  queue_id?: QueueId;
  priority?: Priority;
  /**
   * Unsigned 64-bit byte count encoded as a decimal string (no sign, no leading zeros).
   */
  queue_position?: string;
  category?: FileCategory;
  /**
   * True when a trusted SHA-256 was given with the request and must match before completion (B2.18). The value itself is never echoed. Absent or false otherwise.
   */
  checksum_expected?: boolean;
  /**
   * A bandwidth limit in bytes per second (CORE-06, ADR 0024): a canonical decimal string from 1024 to 1000000000000.
   */
  speed_limit_bytes_per_second?: string;
  scan?: ScanRecord;
  /**
   * Why the current transfer keeps running although its queue's schedule window is closed (CORE-08). Present only while that applies; a pause, cancellation, completion, failure or schedule hold ends it.
   */
  schedule_exemption?: "no_resumable_checkpoint";
  auth?: AuthInfo;
}
/**
 * The OS attachment check of the finished file (F07, E06, ADR 0026). Omitted when none was recorded: before placement, and for downloads finished before this existed (never presented as checked).
 */
export interface ScanRecord {
  status: "scanning" | "attachment_checked" | "blocked" | "unavailable" | "skipped";
  reason?:
    | "policy_blocked"
    | "file_removed"
    | "file_changed"
    | "file_replaced"
    | "service_error"
    | "timed_out"
    | "platform_not_supported"
    | "cancelled";
  origin_marking?: "present" | "absent" | "written_by_downova" | "unsupported_volume" | "not_supported_on_platform";
}
/**
 * A download's scoped login context (BRW-05, ADR 0027): display-safe metadata only, never a cookie or token.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "AuthInfo".
 */
export interface AuthInfo {
  credential_id: string;
  /**
   * scheme://host[:port] only.
   */
  origin: string;
  browser: BrowserKind;
  mode: CredentialMode;
  state: CredentialState;
}
/**
 * Durable evidence of the attachment check (ADR 0026). Never a malware verdict: attachment_checked means Windows Attachment Services processed the file and it was neither removed nor changed afterwards. blocked fails the job (SCAN_BLOCKED). unavailable and skipped complete the job with a visible warning. origin_marking says what is on the file (presence of MOTW proves origin only).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ScanRecord".
 */
export interface ScanRecord1 {
  status: "scanning" | "attachment_checked" | "blocked" | "unavailable" | "skipped";
  reason?:
    | "policy_blocked"
    | "file_removed"
    | "file_changed"
    | "file_replaced"
    | "service_error"
    | "timed_out"
    | "platform_not_supported"
    | "cancelled";
  origin_marking?: "present" | "absent" | "written_by_downova" | "unsupported_volume" | "not_supported_on_platform";
}
/**
 * diagnostics.v1.preview params (E05). include_crash adds the last startup's recovery summary; off unless the user turns it on.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DiagnosticsPreviewParams".
 */
export interface DiagnosticsPreviewParams {
  include_crash?: boolean;
}
/**
 * The sanitized diagnostics report exactly as it would be exported (an allowlist of fields, then a final redaction check). snapshot_id names this content for diagnostics.v1.export.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DiagnosticsPreview".
 */
export interface DiagnosticsPreview {
  snapshot_id: string;
  content: string;
  size_bytes: ByteCount;
}
/**
 * diagnostics.v1.export params: writes a previewed snapshot to the absolute path the user chose. An existing file is refused unless replace is true.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DiagnosticsExportParams".
 */
export interface DiagnosticsExportParams {
  snapshot_id: string;
  path: string;
  replace?: boolean;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DiagnosticsExportResult".
 */
export interface DiagnosticsExportResult {
  size_bytes: ByteCount;
  sha256: string;
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
 * Params of downloads.v1.get / pause / resume / cancel (remove: RemoveParams).
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
 * Params of service.v1.shutdown. {} means keep_state. Unknown fields or modes are rejected and never stop the service.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ShutdownParams".
 */
export interface ShutdownParams {
  mode?: ShutdownMode;
}
/**
 * Acknowledgement of service.v1.shutdown, sent after the paused intent is durable and before the process exits. It does not prove that the process has ended; clients check that separately.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ShutdownResult".
 */
export interface ShutdownResult {
  mode: ShutdownMode;
  /**
   * Unfinished downloads paused by this request (0 for keep_state).
   */
  paused_jobs: number;
}
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
    | "downloads.v1.cancel"
    | "downloads.v1.remove"
    | "downloads.v1.check"
    | "downloads.v1.confirm_name"
    | "downloads.v1.set_queue"
    | "downloads.v1.set_priority"
    | "downloads.v1.move"
    | "settings.v1.get"
    | "settings.v1.set"
    | "concurrency.v1.get"
    | "concurrency.v1.set"
    | "queues.v1.list"
    | "queues.v1.create"
    | "queues.v1.rename"
    | "queues.v1.delete"
    | "queues.v1.set_limit"
    | "queues.v1.start"
    | "queues.v1.stop"
    | "events.v1.subscribe"
    | "history.v1.get"
    | "history.v1.set"
    | "history.v1.preview"
    | "history.v1.clean"
    | "downloads.v1.retry"
    | "downloads.v1.replace_source"
    | "downloads.v1.locate"
    | "notifications.v1.get"
    | "notifications.v1.set"
    | "speed_limits.v1.get"
    | "speed_limits.v1.set"
    | "downloads.v1.set_speed_limit"
    | "queues.v1.set_schedule"
    | "diagnostics.v1.preview"
    | "diagnostics.v1.export"
    | "downloads.v1.add_authenticated"
    | "credentials.v1.list"
    | "credentials.v1.forget"
    | "downloads.v1.reconnect"
    | "credentials.v1.reconnectable"
    | "service.v1.prepare_update"
    | "service.v1.cancel_update";
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
/**
 * Browser native host (B0.6): is a compatible download service reachable?
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostHello".
 */
export interface NativeHostHello {
  v: 1;
  /**
   * Chosen by the extension per message; echoed in the response.
   */
  request_id: string;
  op: "hello";
}
/**
 * Browser native host (B0.6): explicit hand-off of one public http(s) GET link. The host adds destination profile "downloads" and forwards downloads.v1.add with this idempotency key. No cookies, headers, POST bodies or file bytes.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostAdd".
 */
export interface NativeHostAdd {
  v: 1;
  /**
   * Chosen by the extension per message; echoed in the response.
   */
  request_id: string;
  op: "add";
  /**
   * http(s) URL without userinfo; at most 8192 characters. Never logged unredacted.
   */
  url: string;
  idempotency_key: string;
}
/**
 * Download with Downova using my sign-in (BRW-05): after a user action and the cookies permission; the host forwards it as downloads.v1.add_authenticated.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostAddAuthenticated".
 */
export interface NativeHostAddAuthenticated {
  v: 1;
  /**
   * Chosen by the extension per message; echoed in the response.
   */
  request_id: string;
  op: "add_authenticated";
  /**
   * http(s) URL without userinfo; at most 8192 characters. Never logged unredacted.
   */
  url: string;
  idempotency_key: string;
  session: SessionInput;
}
/**
 * The browser session offered for one authenticated download (ADR 0027). remember keeps it in the OS credential store (explicit choice).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SessionInput".
 */
export interface SessionInput {
  /**
   * scheme://host[:port] only.
   */
  origin: string;
  browser: BrowserKind;
  profile_id: string;
  /**
   * @minItems 1
   * @maxItems 64
   */
  cookies: [CookieInput, ...CookieInput[]];
  remember?: boolean;
}
/**
 * One cookie from the browser's own cookie store for the link (ingress only: extension → native host → service; never in a response).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CookieInput".
 */
export interface CookieInput {
  name: string;
  value: string;
  domain: string;
  host_only: boolean;
  path: string;
  secure: boolean;
  expires_at_ms?: ByteCount;
}
/**
 * ADR 0028: after the user's 'Reconnect a Downova download with my sign-in' action, which of this origin's downloads can be reconnected. No cookie is sent.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostReconnectCandidates".
 */
export interface NativeHostReconnectCandidates {
  v: 1;
  /**
   * Chosen by the extension per message; echoed in the response.
   */
  request_id: string;
  op: "reconnect_candidates";
  /**
   * scheme://host[:port] only.
   */
  origin: string;
  browser: BrowserKind;
  profile_id: string;
}
/**
 * ADR 0028: reconnect the download the user selected with the browser's current sign-in; forwarded as downloads.v1.reconnect.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostReconnect".
 */
export interface NativeHostReconnect {
  v: 1;
  /**
   * Chosen by the extension per message; echoed in the response.
   */
  request_id: string;
  op: "reconnect";
  job_id: JobId;
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  expected_credential_id: string;
  session: SessionInput;
  resume?: boolean;
}
/**
 * Native host reply. request_id is null only when the request could not be parsed.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NativeHostResponse".
 */
export interface NativeHostResponse {
  v: 1;
  request_id: string | null;
  outcome: NativeHostOutcome;
  job?: DownloadSnapshot | null;
  hello?: HelloResult | null;
  error?: ErrorData | null;
  /**
   * The answer to reconnect_candidates (metadata only).
   */
  candidates?: ReconnectCandidate[];
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReconnectCandidate".
 */
export interface ReconnectCandidate {
  job_id: JobId;
  display_name: string;
  phase: Phase;
  desired_state: DesiredState;
  downloaded_bytes: ByteCount;
  total_bytes?: ByteCount;
  auth: AuthInfo;
}
/**
 * Params of downloads.v1.check (B2.8).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CheckLinkParams".
 */
export interface CheckLinkParams {
  source: DownloadSource;
}
/**
 * An unremoved job with the same link identity. Never carries the URL.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DuplicateSummary".
 */
export interface DuplicateSummary {
  job_id: JobId;
  display_name: string;
  phase: Phase;
  desired_state: DesiredState;
}
/**
 * Result of downloads.v1.check: same-link jobs, oldest first. Link identity is the exact URL with only scheme and host case-folded; redacted URLs are never compared.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CheckLinkResult".
 */
export interface CheckLinkResult {
  duplicates: DuplicateSummary[];
}
/**
 * Category to folder mapping. The folder is an absolute, existing local directory without . or .. components.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DestinationRule".
 */
export interface DestinationRule {
  category: FileCategory;
  directory: string;
}
/**
 * settings.v1.get result and settings.v1.set params (replaces the whole value). Rule edits never move existing jobs.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ServiceSettings".
 */
export interface ServiceSettings {
  /**
   * @maxItems 6
   */
  destination_rules:
    | []
    | [DestinationRule]
    | [DestinationRule, DestinationRule]
    | [DestinationRule, DestinationRule, DestinationRule]
    | [DestinationRule, DestinationRule, DestinationRule, DestinationRule]
    | [DestinationRule, DestinationRule, DestinationRule, DestinationRule, DestinationRule]
    | [DestinationRule, DestinationRule, DestinationRule, DestinationRule, DestinationRule, DestinationRule];
  on_collision: CollisionPolicy;
}
/**
 * speed_limits.v1.get result and speed_limits.v1.set params (CORE-06, ADR 0024): the limit shared by all downloads. The field is required; null means unlimited (the default). Saved before it takes effect; a refused save changes nothing.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SpeedLimitSettings".
 */
export interface SpeedLimitSettings {
  global_bytes_per_second: SpeedLimit;
}
/**
 * downloads.v1.set_speed_limit params (CORE-06): one download's own limit, shared by all of its segments; null removes it (only the global limit applies). Applied live to a running transfer.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SetJobSpeedLimitParams".
 */
export interface SetJobSpeedLimitParams {
  job_id: JobId;
  bytes_per_second: SpeedLimit;
}
/**
 * Global active-download admission (ADR 0023). Persisted separately from destination settings. Default 3, range 1–16. Queue limits and the host connection budget still apply. Lowering never preempts transfers; raising admits eligible jobs immediately.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ConcurrencySettings".
 */
export interface ConcurrencySettings {
  max_active_downloads: number;
}
/**
 * A download queue (B2.11).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "QueueInfo".
 */
export interface QueueInfo {
  queue_id: QueueId;
  name: string;
  position: number;
  max_active: number;
  /**
   * False after queues.v1.stop: its jobs hold with queue_stopped.
   */
  running: boolean;
  schedule?: QueueSchedule;
  schedule_state?: ScheduleState;
}
/**
 * A queue's schedule (CORE-08, ADR 0025). Outside its windows the queue's unfinished jobs hold with schedule_window: a policy blocker that never resumes a manual pause, never overrides a stopped queue and never bypasses limits. timezone is an IANA name. DST: a boundary in a gap moves to the first instant after it; in a repeated hour a start takes the earlier and an end the later occurrence. missed_window: skip (default) or catch_up (one bounded catch-up for the most recent window missed while the service was not running).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "QueueSchedule".
 */
export interface QueueSchedule {
  enabled: boolean;
  timezone: string;
  /**
   * @minItems 1
   * @maxItems 14
   */
  windows:
    | [ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow, ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow]
    | [ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow, ScheduleWindow]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ]
    | [
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow,
        ScheduleWindow
      ];
  missed_window?: "skip" | "catch_up";
}
/**
 * One recurring window (CORE-08): on these days, from start to end in the schedule's zone (HH:MM). end earlier than start crosses midnight and belongs to the start day; start equal to end is refused. Windows are half-open: in at start, out at end.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ScheduleWindow".
 */
export interface ScheduleWindow {
  /**
   * @minItems 1
   * @maxItems 7
   */
  days:
    | [Weekday]
    | [Weekday, Weekday]
    | [Weekday, Weekday, Weekday]
    | [Weekday, Weekday, Weekday, Weekday]
    | [Weekday, Weekday, Weekday, Weekday, Weekday]
    | [Weekday, Weekday, Weekday, Weekday, Weekday, Weekday]
    | [Weekday, Weekday, Weekday, Weekday, Weekday, Weekday, Weekday];
  start: string;
  end: string;
}
/**
 * A schedule's state as the service evaluated it (CORE-08). Instants are UTC RFC 3339.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ScheduleState".
 */
export interface ScheduleState {
  in_window: boolean;
  next_change_at: string | null;
  catch_up_until: string | null;
}
/**
 * queues.v1.set_schedule params (CORE-08): saved before it applies; null removes the schedule. A refused save changes nothing.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SetQueueScheduleParams".
 */
export interface SetQueueScheduleParams {
  queue_id: QueueId;
  schedule: QueueSchedule | null;
}
/**
 * Result of queues.v1.list, in list order.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ListQueuesResult".
 */
export interface ListQueuesResult {
  queues: QueueInfo[];
}
/**
 * Params of queues.v1.create.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CreateQueueParams".
 */
export interface CreateQueueParams {
  name: string;
  max_active?: number;
}
/**
 * Params of queues.v1.start / queues.v1.stop.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "QueueRef".
 */
export interface QueueRef {
  queue_id: QueueId;
}
/**
 * Params of queues.v1.rename.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RenameQueueParams".
 */
export interface RenameQueueParams {
  queue_id: QueueId;
  name: string;
}
/**
 * Params of queues.v1.set_limit. Lowering never stops a running transfer.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SetQueueLimitParams".
 */
export interface SetQueueLimitParams {
  queue_id: QueueId;
  max_active: number;
}
/**
 * Params of queues.v1.delete. Never deletes jobs or files: a queue with jobs needs move_to; 'main' cannot be deleted.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "DeleteQueueParams".
 */
export interface DeleteQueueParams {
  queue_id: QueueId;
  move_to?: QueueId;
}
/**
 * Params of downloads.v1.set_queue: the job moves to the end of the queue.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SetJobQueueParams".
 */
export interface SetJobQueueParams {
  job_id: JobId;
  queue_id: QueueId;
}
/**
 * Params of downloads.v1.set_priority.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SetPriorityParams".
 */
export interface SetPriorityParams {
  job_id: JobId;
  priority: Priority;
}
/**
 * Params of downloads.v1.move: manual execution order inside the job's queue.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "MoveJobParams".
 */
export interface MoveJobParams {
  job_id: JobId;
  to: MoveTarget;
}
/**
 * Result of events.v1.subscribe (B2.9): an authoritative list and the sequence it reflects. Each later events.v1.event on the same connection has sequence = previous + 1; a gap, snapshot_required or another service_instance_id means: subscribe again.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "SubscribeResult".
 */
export interface SubscribeResult {
  service_instance_id: string;
  sequence: ByteCount;
  items: DownloadSnapshot[];
}
/**
 * Server-to-client notification on a subscribed connection (no id, no answer).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RpcNotification".
 */
export interface RpcNotification {
  jsonrpc: "2.0";
  method: "events.v1.event";
  params: ServiceEvent;
}
/**
 * history.v1.get result and history.v1.set params (B2.12, ADR 0018). Finished jobs (completed, failed, cancelled) leave the list retention_days after they finished; null (the default) keeps them indefinitely. Downloaded and partial files are never touched. Separate from settings.v1 so clients that save destination settings cannot change it.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "HistorySettings".
 */
export interface HistorySettings {
  /**
   * Days after a finished job finished; null keeps history indefinitely.
   */
  retention_days: number | null;
}
/**
 * Params of history.v1.preview: the policy to evaluate now (not saved).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "HistoryPreviewParams".
 */
export interface HistoryPreviewParams {
  /**
   * Days after a finished job finished; null keeps history indefinitely.
   */
  retention_days: number | null;
}
/**
 * Result of history.v1.preview: listed finished jobs the policy would remove now (expiring) and all listed finished jobs (finished).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "HistoryPreview".
 */
export interface HistoryPreview {
  expiring: number;
  finished: number;
}
/**
 * Result of history.v1.clean: jobs removed from the list now (removed) and eligible jobs kept because their removal could not be recorded (kept; retried later).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "HistoryCleanResult".
 */
export interface HistoryCleanResult {
  removed: number;
  kept: number;
}
/**
 * Params of downloads.v1.remove (B2.3, ADR 0013): takes a completed, failed or cancelled download off the list. The file is kept unless delete_file is true; then, for a completed download only, the service first deletes its own finished file (a regular file under the job's destination with the job's file name; never a folder or a link) and keeps the job in the list if that fails (DESTINATION_UNWRITABLE). A file that is already gone is not an error. Result: DownloadSnapshot.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RemoveParams".
 */
export interface RemoveParams {
  job_id: JobId;
  /**
   * Also delete the downloaded file (completed downloads only). Default false.
   */
  delete_file?: boolean;
}
/**
 * Params of downloads.v1.retry (B2.16, ADR 0020): a fresh attempt for a failed job. A failed job has no reusable resume point, so Retry starts from zero; discard_partial must be true when the job holds partial data (otherwise refused with next_action choose_restart_or_keep). Result: DownloadSnapshot.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "RetryParams".
 */
export interface RetryParams {
  job_id: JobId;
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  /**
   * The attempt the client saw (attempt_id of its snapshot). When given and different from the current attempt, the request is refused (the job changed).
   */
  expected_attempt_id?: AttemptId | null;
  discard_partial?: boolean;
}
/**
 * Params of downloads.v1.replace_source (B2.17, ADR 0020) for a paused or failed job. The url is never echoed, logged or published.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReplaceSourceParams".
 */
export interface ReplaceSourceParams {
  job_id: JobId;
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  url: string;
  mode: ReplaceMode;
  /**
   * The attempt the client saw (attempt_id of its snapshot). When given and different from the current attempt, the request is refused (the job changed).
   */
  expected_attempt_id?: AttemptId | null;
}
/**
 * Result of downloads.v1.replace_source.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReplaceSourceResult".
 */
export interface ReplaceSourceResult {
  outcome: ReplaceOutcome;
  reason: ReplaceReason | null;
  job: DownloadSnapshot;
}
/**
 * Result of downloads.v1.locate (B2.19; params JobRef): the job's bound folder, and its final file name once completed (empty otherwise). Used only for a user-triggered Open containing folder.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "LocateResult".
 */
export interface LocateResult {
  directory: string;
  file_name: string;
  directory_present: boolean;
  file_present: boolean;
}
/**
 * Params of notifications.v1.set (CORE-09, ADR 0022). Off by default. Turning it on is the explicit user action that shows one confirmation notification (where the system asks for permission, it asks then).
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NotificationSettings".
 */
export interface NotificationSettings {
  enabled: boolean;
  locale: NotificationLocale;
}
/**
 * Result of notifications.v1.get / set (params of get: EmptyResult). Only completed (verified and placed) and failed outcomes notify, at most once each; cancellations and anything that finished before notifications were on do not.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "NotificationStatus".
 */
export interface NotificationStatus {
  enabled: boolean;
  locale: NotificationLocale;
  availability: NotificationAvailability;
  /**
   * The last hand-off to the system failed (for example, blocked there). Never retried.
   */
  last_delivery_failed: boolean;
}
/**
 * downloads.v1.add_authenticated (BRW-05): a GET link with a scoped browser session. Refused unless the backend reports authenticated_get supported and the link is https and of the session's origin.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "AddAuthenticatedParams".
 */
export interface AddAuthenticatedParams {
  add: AddDownloadParams;
  session: SessionInput;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CredentialInfo".
 */
export interface CredentialInfo {
  credential_id: string;
  /**
   * scheme://host[:port] only.
   */
  origin: string;
  browser: BrowserKind;
  mode: CredentialMode;
  state: CredentialState;
  created_at_ms: ByteCount;
  expires_at_ms?: ByteCount;
  active_jobs: number;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ListCredentialsResult".
 */
export interface ListCredentialsResult {
  credentials: CredentialInfo[];
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ForgetCredentialParams".
 */
export interface ForgetCredentialParams {
  credential_id: string;
}
/**
 * downloads.v1.reconnect (ADR 0028): attach a fresh browser sign-in to one existing signed-in download. Bound to the job, its origin, the browser profile and expected_credential_id (the job's current credential; every reconnect gives a new one, so a stale request is refused). operation_id makes retries idempotent: a repeat answers with the current snapshot and changes nothing again. resume is the explicit 'Reconnect and resume'; without it a manual pause or a stopped queue is kept. Result: DownloadSnapshot.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReconnectParams".
 */
export interface ReconnectParams {
  job_id: JobId;
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  expected_credential_id: string;
  session: SessionInput;
  resume?: boolean;
}
/**
 * credentials.v1.reconnectable (ADR 0028): the unfinished signed-in downloads of one origin that this browser profile can reconnect. Metadata only.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReconnectableParams".
 */
export interface ReconnectableParams {
  /**
   * scheme://host[:port] only.
   */
  origin: string;
  browser: BrowserKind;
  profile_id: string;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "ReconnectableResult".
 */
export interface ReconnectableResult {
  downloads: ReconnectCandidate[];
}
/**
 * service.v1.prepare_update (ADV09, ADR 0030): quiesce for an already verified update. The service holds a bounded lease: it stops admitting transfers, commits the running ones' checkpoints without changing any desired state, and writes and checks a backup. Any failure releases the lease and changes nothing. Result: PrepareUpdateResult.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "PrepareUpdateParams".
 */
export interface PrepareUpdateParams {
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  target_version: string;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "UpdateBackupInfo".
 */
export interface UpdateBackupInfo {
  schema_version: number;
  jobs: ByteCount;
  size_bytes: ByteCount;
  sha256: string;
  created_at_ms: ByteCount;
}
/**
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "PrepareUpdateResult".
 */
export interface PrepareUpdateResult {
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
  source_version: string;
  target_version: string;
  backup: UpdateBackupInfo;
  held_jobs: number;
  lease_expires_at_ms: ByteCount;
}
/**
 * service.v1.cancel_update: ends the update lease; transfers held for it continue. Result: {}.
 *
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "CancelUpdateParams".
 */
export interface CancelUpdateParams {
  /**
   * Client-chosen identity of this operation (8–128 of A-Z a-z 0-9 _ -). A repeat answers with the recorded outcome and never starts another attempt; the same id with a different request is IDEMPOTENCY_CONFLICT.
   */
  operation_id: string;
}
