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
  | "queue_limit"
  | "queue_stopped"
  | "name_conflict";
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
  | "metalink";
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
export type NativeHostRequest = NativeHostHello | NativeHostAdd;
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
 * This interface was referenced by `ServiceApiV1`'s JSON-Schema
 * via the `definition` "MoveTarget".
 */
export type MoveTarget = "up" | "down" | "top" | "bottom";

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
 * Params of downloads.v1.get / pause / resume / cancel / remove.
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
    | "history.v1.clean";
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
