// Service-client boundary: the ONLY module family in the UI allowed to reach the desktop shell
// (Tauri) and, through it, the background service. Features depend on the `ServiceClient`
// interface, never on transport details. Contract types come from @dm/contracts.
import type {
  ConcurrencySettings,
  DiagnosticsPreview,
  DownloadSnapshot,
  DuplicatePolicy,
  DuplicateSummary,
  ErrorData,
  FileCategory,
  HelloResult,
  HistoryCleanResult,
  HistoryPreview,
  HistorySettings,
  JobId,
  ListCredentialsResult,
  ListQueuesResult,
  MoveTarget,
  NotificationLocale,
  NotificationStatus,
  PrepareUpdateResult,
  Priority,
  QueueInfo,
  QueueSchedule,
  ReplaceMode,
  ReplaceSourceResult,
  ServiceEvent,
  ProxySettings,
  ServiceSettings,
  SetProxyParams,
  SpeedLimitSettings,
  SubscribeResult,
} from "@dm/contracts";
import { invoke, isTauri } from "@tauri-apps/api/core";

/** One browser package's support verdict (BRW-09, ADR 0029): stable codes, translated by the UI. */
export interface BrowserVerdict {
  package: "native" | "snap" | "flatpak" | "unknown";
  support: "supported" | "unsupported" | "untested";
  reason: string;
}

/** One browser's connection to this installation (the shell's answer; never a path). */
export interface BrowserConnection {
  browser: string;
  /** ours, absent, stale, other_installation, unreadable, foreign or error. */
  registration: string;
  packages: string[];
  support: BrowserVerdict[];
}

export type BrowserRepairOutcome =
  | { outcome: "repaired"; connection: BrowserConnection }
  | { outcome: "refused"; reason: string; connection: BrowserConnection }
  | { outcome: "failed"; reason: string };

/** What became of a diagnostics export (the shell's answer; the path stays in the shell). */
export type DiagnosticsExportOutcome =
  | { outcome: "cancelled" }
  | { outcome: "written"; file_name: string; size_bytes: string; sha256: string };
import { listen } from "@tauri-apps/api/event";

export type ConnectionState =
  | { status: "connected"; hello: HelloResult }
  | {
      status: "unavailable";
      /** Why the UI cannot talk to the service. Presented to the user honestly. */
      reason: "not_implemented" | "not_running" | "incompatible_version" | "list_unreadable";
      error: ErrorData | null;
    };

/**
 * What the UI may assume about a failed call (the shell's `BridgeError.outcome`):
 * - `unavailable`: nothing reached the service;
 * - `uncertain`: a mutation was sent but not acknowledged — it may have happened. An add must be
 *   repeated with the SAME idempotency key, and only when the user asks;
 * - `rejected`: the service (or the shell's argument checks) refused it.
 */
export type CallOutcome = "unavailable" | "uncertain" | "rejected";

export class ServiceCallError extends Error {
  constructor(
    readonly outcome: CallOutcome,
    readonly error: ErrorData,
  ) {
    super(`${outcome}: ${error.kind}`);
    this.name = "ServiceCallError";
  }
}

/** Optional add choices (B2.8). Omitted fields keep the behaviour from before B2.8. */
export interface AddOptions {
  category?: FileCategory;
  onDuplicate?: DuplicatePolicy;
  queueId?: string;
  /** Trusted publisher SHA-256 (B2.18), 64 hex digits. Omitted when absent. */
  expectedSha256?: string;
}

/** downloads.v1.retry choices (B2.16). */
export interface RetryOptions {
  /** The attempt the user saw; the service refuses when the job moved on meanwhile. */
  expectedAttemptId: string | null;
  /** Confirms that the partial data is discarded (Retry always starts from zero). */
  discardPartial: boolean;
}

/** What "Open containing folder" did (the shell's `folders::Revealed`). */
export type Revealed = "file" | "folder" | "folder_file_missing";

/** Why an event stream stopped (the shell's `STREAM_ENDED` reasons). */
export type StreamEnd = "closed" | "snapshot_required" | "lost";

export interface StreamHandlers {
  onEvent(event: ServiceEvent): void;
  onEnded(reason: StreamEnd): void;
}

/** An open subscription: the authoritative start, and a way to stop listening. */
export interface Subscription {
  start: SubscribeResult;
  close(): void;
}

export interface ServiceClient {
  /** Finds (or, in the desktop app, starts) the service. Never throws for expected unavailability. */
  connect(): Promise<ConnectionState>;
  /** Looks for a running service without ever starting it (automatic reconnection, B2.10). */
  probe(): Promise<ConnectionState>;
  /**
   * events.v1.subscribe (B2.9): resolves with a snapshot and its sequence; events follow through
   * `handlers` until the stream ends. Rejects with a ServiceCallError; `rejected` means this
   * service offers no events (an older service), so the caller may poll instead.
   */
  subscribe(handlers: StreamHandlers): Promise<Subscription>;
  /** downloads.v1.list. Rejects with a ServiceCallError. */
  listDownloads(): Promise<DownloadSnapshot[]>;
  /** downloads.v1.add for one http(s) link with the caller's idempotency key. */
  addDownload(url: string, idempotencyKey: string, options?: AddOptions): Promise<DownloadSnapshot>;
  pauseDownload(jobId: JobId): Promise<DownloadSnapshot>;
  resumeDownload(jobId: JobId): Promise<DownloadSnapshot>;
  cancelDownload(jobId: JobId): Promise<DownloadSnapshot>;
  /** downloads.v1.remove: a finished job leaves the list (ADR 0013). Never touches files. */
  /** Off the list; with `deleteFile` a completed download's file is deleted first. */
  removeDownload(jobId: JobId, options?: { deleteFile?: boolean }): Promise<DownloadSnapshot>;
  /** downloads.v1.check: unremoved jobs with the same link (summaries never carry the URL). */
  checkLink(url: string): Promise<DuplicateSummary[]>;
  /** downloads.v1.confirm_name: accept the free name of a job held by `name_conflict`. */
  confirmName(jobId: JobId): Promise<DownloadSnapshot>;
  setJobQueue(jobId: JobId, queueId: string): Promise<DownloadSnapshot>;
  setPriority(jobId: JobId, priority: Priority): Promise<DownloadSnapshot>;
  /** Manual execution order inside the job's queue (not a presentation sort). */
  moveJob(jobId: JobId, to: MoveTarget): Promise<DownloadSnapshot>;
  getSettings(): Promise<ServiceSettings>;
  setSettings(settings: ServiceSettings): Promise<ServiceSettings>;
  getConcurrency(): Promise<ConcurrencySettings>;
  setConcurrency(maxActiveDownloads: number): Promise<ConcurrencySettings>;
  /** CORE-06: the overall bandwidth limit, bytes per second as a decimal string, or null. */
  /** BRW-07/09 (ADR 0029): every browser's connection, package and support verdict. */
  browserConnections(): Promise<BrowserConnection[]>;
  /** Repairs one browser's registration for this installation (never another's). */
  repairBrowser(browser: string): Promise<BrowserRepairOutcome>;
  /** SEC-02 (ADR 0027): sign-ins by id, origin, mode and state; never a secret. */
  listCredentials(): Promise<ListCredentialsResult>;
  /** Forgets a sign-in (memory and the OS store); downloads using it pause. */
  forgetCredential(credentialId: string): Promise<ListCredentialsResult>;
  /** E05: the redacted diagnostics report as it would be exported (nothing is written). */
  previewDiagnostics(includeCrash: boolean): Promise<DiagnosticsPreview>;
  /** E05: the native save dialog (in the shell) and the export of that snapshot. */
  exportDiagnostics(snapshotId: string, title: string): Promise<DiagnosticsExportOutcome>;
  getSpeedLimit(): Promise<SpeedLimitSettings>;
  /** `proxy.v1.get` (F06, ADR 0034): never returns the password. */
  getProxy(): Promise<ProxySettings>;
  /** `proxy.v1.set`: a password is sent once and goes only to the OS credential store. */
  setProxy(params: SetProxyParams): Promise<ProxySettings>;
  setSpeedLimit(bytesPerSecond: string | null): Promise<SpeedLimitSettings>;
  /** CORE-06: one download's own limit (shared by its segments), or null for none. */
  setDownloadSpeedLimit(jobId: JobId, bytesPerSecond: string | null): Promise<DownloadSnapshot>;
  listQueues(): Promise<QueueInfo[]>;
  createQueue(name: string, maxActive?: number): Promise<QueueInfo>;
  renameQueue(queueId: string, name: string): Promise<QueueInfo>;
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo>;
  /** CORE-08: a queue's schedule, or null to remove it. */
  setQueueSchedule(queueId: string, schedule: QueueSchedule | null): Promise<QueueInfo>;
  /** Never deletes jobs or files: a queue that still has jobs needs `moveTo`. */
  deleteQueue(queueId: string, moveTo?: string): Promise<void>;
  startQueue(queueId: string): Promise<QueueInfo>;
  stopQueue(queueId: string): Promise<QueueInfo>;
  /** history.v1.* (B2.12): retention of finished list entries; files are never touched. */
  getHistory(): Promise<HistorySettings>;
  setHistory(retentionDays: number | null): Promise<HistorySettings>;
  previewHistory(retentionDays: number | null): Promise<HistoryPreview>;
  cleanHistory(): Promise<HistoryCleanResult>;
  /** downloads.v1.retry (B2.16). An uncertain answer is repeated with the SAME operation ID. */
  retryDownload(
    jobId: JobId,
    operationId: string,
    options: RetryOptions,
  ): Promise<DownloadSnapshot>;
  /** downloads.v1.replace_source (B2.17): Refresh link. */
  replaceSource(
    jobId: JobId,
    operationId: string,
    url: string,
    mode: ReplaceMode,
    expectedAttemptId: string | null,
  ): Promise<ReplaceSourceResult>;
  /**
   * Open containing folder (B2.19): the shell resolves the folder through the service and only
   * ever reveals it (a file is selected, never opened). Only after a user action.
   */
  revealDownload(jobId: JobId): Promise<Revealed>;
  /** notifications.v1.* (CORE-09): the service shows them, also with the window closed. */
  getNotifications(): Promise<NotificationStatus>;
  setNotifications(enabled: boolean, locale: NotificationLocale): Promise<NotificationStatus>;
}

const OUTCOMES: readonly CallOutcome[] = ["unavailable", "uncertain", "rejected"];

/**
 * Normalizes whatever a call threw. The shell rejects with `{ outcome, error }`; anything else
 * (a missing command, a broken bridge) is treated as unavailable for reads and, because the
 * command may have run, as uncertain for mutations.
 */
export function toCallError(thrown: unknown, mutation = false): ServiceCallError {
  if (thrown instanceof ServiceCallError) return thrown;
  if (typeof thrown === "object" && thrown !== null) {
    const { outcome, error } = thrown as { outcome?: unknown; error?: unknown };
    if (
      typeof outcome === "string" &&
      (OUTCOMES as readonly string[]).includes(outcome) &&
      typeof error === "object" &&
      error !== null &&
      typeof (error as ErrorData).kind === "string"
    ) {
      return new ServiceCallError(outcome as CallOutcome, error as ErrorData);
    }
  }
  return new ServiceCallError(mutation ? "uncertain" : "unavailable", {
    kind: "SERVICE_UNAVAILABLE",
    retryable: true,
    next_action: "retry",
    detail: null,
  });
}

const NOT_IMPLEMENTED: ErrorData = {
  kind: "SERVICE_UNAVAILABLE",
  retryable: false,
  next_action: "none",
  detail: null,
};

const NOT_RUNNING: ConnectionState = {
  status: "unavailable",
  reason: "not_running",
  error: { kind: "SERVICE_UNAVAILABLE", retryable: true, next_action: "retry", detail: null },
};

/**
 * Plain-browser client (`pnpm dev`): there is no desktop shell and therefore no service bridge.
 * It reports `not_implemented` and refuses every call instead of pretending to connect.
 */
export class FoundationServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    return { status: "unavailable", reason: "not_implemented", error: NOT_IMPLEMENTED };
  }
  probe(): Promise<ConnectionState> {
    return this.connect();
  }
  private refuse(): Promise<never> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  subscribe = (): Promise<Subscription> => this.refuse();
  listDownloads = (): Promise<DownloadSnapshot[]> => this.refuse();
  addDownload = (): Promise<DownloadSnapshot> => this.refuse();
  pauseDownload = (): Promise<DownloadSnapshot> => this.refuse();
  resumeDownload = (): Promise<DownloadSnapshot> => this.refuse();
  cancelDownload = (): Promise<DownloadSnapshot> => this.refuse();
  removeDownload = (): Promise<DownloadSnapshot> => this.refuse();
  checkLink = (): Promise<DuplicateSummary[]> => this.refuse();
  confirmName = (): Promise<DownloadSnapshot> => this.refuse();
  setJobQueue = (): Promise<DownloadSnapshot> => this.refuse();
  setPriority = (): Promise<DownloadSnapshot> => this.refuse();
  moveJob = (): Promise<DownloadSnapshot> => this.refuse();
  getSettings = (): Promise<ServiceSettings> => this.refuse();
  setSettings = (): Promise<ServiceSettings> => this.refuse();
  getConcurrency = (): Promise<ConcurrencySettings> => this.refuse();
  setConcurrency = (): Promise<ConcurrencySettings> => this.refuse();
  browserConnections = (): Promise<BrowserConnection[]> => this.refuse();
  repairBrowser = (): Promise<BrowserRepairOutcome> => this.refuse();
  listCredentials = (): Promise<ListCredentialsResult> => this.refuse();
  forgetCredential = (): Promise<ListCredentialsResult> => this.refuse();
  previewDiagnostics = (): Promise<DiagnosticsPreview> => this.refuse();
  exportDiagnostics = (): Promise<DiagnosticsExportOutcome> => this.refuse();
  getSpeedLimit = (): Promise<SpeedLimitSettings> => this.refuse();
  getProxy = (): Promise<ProxySettings> => this.refuse();
  setProxy = (): Promise<ProxySettings> => this.refuse();
  setSpeedLimit = (): Promise<SpeedLimitSettings> => this.refuse();
  setDownloadSpeedLimit = (): Promise<DownloadSnapshot> => this.refuse();
  listQueues = (): Promise<QueueInfo[]> => this.refuse();
  createQueue = (): Promise<QueueInfo> => this.refuse();
  renameQueue = (): Promise<QueueInfo> => this.refuse();
  setQueueLimit = (): Promise<QueueInfo> => this.refuse();
  setQueueSchedule = (): Promise<QueueInfo> => this.refuse();
  deleteQueue = (): Promise<void> => this.refuse();
  startQueue = (): Promise<QueueInfo> => this.refuse();
  stopQueue = (): Promise<QueueInfo> => this.refuse();
  getHistory = (): Promise<HistorySettings> => this.refuse();
  setHistory = (): Promise<HistorySettings> => this.refuse();
  previewHistory = (): Promise<HistoryPreview> => this.refuse();
  cleanHistory = (): Promise<HistoryCleanResult> => this.refuse();
  retryDownload = (): Promise<DownloadSnapshot> => this.refuse();
  replaceSource = (): Promise<ReplaceSourceResult> => this.refuse();
  revealDownload = (): Promise<Revealed> => this.refuse();
  getNotifications = (): Promise<NotificationStatus> => this.refuse();
  setNotifications = (): Promise<NotificationStatus> => this.refuse();
}

type StreamItem = { subscription: number; event?: ServiceEvent; reason?: StreamEnd };

/**
 * Desktop client: the shell's `service_connect` finds or starts the per-user service; every other
 * command forwards one fixed service operation (validated in Rust, SEC-05).
 */
export class TauriServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    try {
      return await invoke<ConnectionState>("service_connect");
    } catch {
      return NOT_RUNNING;
    }
  }

  async probe(): Promise<ConnectionState> {
    try {
      return await invoke<ConnectionState>("service_probe");
    } catch {
      return NOT_RUNNING;
    }
  }

  /**
   * Listens before subscribing so that no event is missed. Only events tagged with this
   * subscription's number are delivered; those arriving before the number is known are buffered.
   */
  async subscribe(handlers: StreamHandlers): Promise<Subscription> {
    let mine: number | null = null;
    let closed = false;
    const early: StreamItem[] = [];
    const deliver = (item: StreamItem) => {
      if (closed || item.subscription !== mine) return;
      if (item.event) handlers.onEvent(item.event);
      if (item.reason) {
        closed = true;
        handlers.onEnded(item.reason);
      }
    };
    const accept = (item: StreamItem) => (mine === null ? early.push(item) : deliver(item));
    const offEvent = await listen<StreamItem>(SERVICE_EVENT, (e) => accept(e.payload));
    const offEnded = await listen<StreamItem>(STREAM_ENDED, (e) => accept(e.payload));
    const stop = () => {
      closed = true;
      offEvent();
      offEnded();
    };
    try {
      const opened = await invoke<{ subscription: number; start: SubscribeResult }>(
        "events_subscribe",
      );
      mine = opened.subscription;
      // The caller applies the start first; buffered events follow right after.
      queueMicrotask(() => {
        for (const item of early.splice(0)) deliver(item);
      });
      return {
        start: opened.start,
        close: () => {
          stop();
          void invoke("events_unsubscribe").catch(() => {});
        },
      };
    } catch (e) {
      stop();
      throw toCallError(e);
    }
  }

  async listDownloads(): Promise<DownloadSnapshot[]> {
    return (await read<{ items: DownloadSnapshot[] }>("downloads_list")).items;
  }

  addDownload(
    url: string,
    idempotencyKey: string,
    options: AddOptions = {},
  ): Promise<DownloadSnapshot> {
    // Unset options are not sent, so the service behaves exactly as before B2.8.
    const args: Record<string, unknown> = { url, idempotencyKey };
    if (options.category !== undefined) args.category = options.category;
    if (options.onDuplicate !== undefined) args.onDuplicate = options.onDuplicate;
    if (options.queueId !== undefined) args.queueId = options.queueId;
    if (options.expectedSha256 !== undefined) args.expectedSha256 = options.expectedSha256;
    return mutate("downloads_add", args);
  }
  pauseDownload(jobId: JobId): Promise<DownloadSnapshot> {
    return mutate("downloads_pause", { jobId });
  }
  resumeDownload(jobId: JobId): Promise<DownloadSnapshot> {
    return mutate("downloads_resume", { jobId });
  }
  cancelDownload(jobId: JobId): Promise<DownloadSnapshot> {
    return mutate("downloads_cancel", { jobId });
  }
  removeDownload(jobId: JobId, options?: { deleteFile?: boolean }): Promise<DownloadSnapshot> {
    return mutate("downloads_remove", { jobId, deleteFile: options?.deleteFile === true });
  }
  async checkLink(url: string): Promise<DuplicateSummary[]> {
    return (await read<{ duplicates: DuplicateSummary[] }>("downloads_check", { url })).duplicates;
  }
  confirmName(jobId: JobId): Promise<DownloadSnapshot> {
    return mutate("downloads_confirm_name", { jobId });
  }
  setJobQueue(jobId: JobId, queueId: string): Promise<DownloadSnapshot> {
    return mutate("downloads_set_queue", { jobId, queueId });
  }
  setPriority(jobId: JobId, priority: Priority): Promise<DownloadSnapshot> {
    return mutate("downloads_set_priority", { jobId, priority });
  }
  moveJob(jobId: JobId, to: MoveTarget): Promise<DownloadSnapshot> {
    return mutate("downloads_move", { jobId, to });
  }
  getSettings(): Promise<ServiceSettings> {
    return read<ServiceSettings>("service_settings_get");
  }
  setSettings(settings: ServiceSettings): Promise<ServiceSettings> {
    return mutate<ServiceSettings>("service_settings_set", { settings });
  }
  async listQueues(): Promise<QueueInfo[]> {
    return (await read<ListQueuesResult>("queues_list")).queues;
  }
  getConcurrency(): Promise<ConcurrencySettings> {
    return read<ConcurrencySettings>("concurrency_get");
  }
  setConcurrency(maxActiveDownloads: number): Promise<ConcurrencySettings> {
    return mutate<ConcurrencySettings>("concurrency_set", { maxActiveDownloads });
  }
  browserConnections(): Promise<BrowserConnection[]> {
    return read<BrowserConnection[]>("browser_connections");
  }
  repairBrowser(browser: string): Promise<BrowserRepairOutcome> {
    return mutate<BrowserRepairOutcome>("browser_repair", { browser });
  }
  listCredentials(): Promise<ListCredentialsResult> {
    return read<ListCredentialsResult>("credentials_list");
  }
  forgetCredential(credentialId: string): Promise<ListCredentialsResult> {
    return mutate<ListCredentialsResult>("credentials_forget", { credentialId });
  }
  previewDiagnostics(includeCrash: boolean): Promise<DiagnosticsPreview> {
    return read<DiagnosticsPreview>("diagnostics_preview", { includeCrash });
  }
  exportDiagnostics(snapshotId: string, title: string): Promise<DiagnosticsExportOutcome> {
    return mutate<DiagnosticsExportOutcome>("diagnostics_export", { snapshotId, title });
  }
  getSpeedLimit(): Promise<SpeedLimitSettings> {
    return read<SpeedLimitSettings>("speed_limit_get");
  }
  getProxy(): Promise<ProxySettings> {
    return read<ProxySettings>("proxy_get");
  }
  setProxy(params: SetProxyParams): Promise<ProxySettings> {
    return mutate<ProxySettings>("proxy_set", { params });
  }
  setSpeedLimit(bytesPerSecond: string | null): Promise<SpeedLimitSettings> {
    return mutate<SpeedLimitSettings>("speed_limit_set", { bytesPerSecond });
  }
  setDownloadSpeedLimit(jobId: JobId, bytesPerSecond: string | null): Promise<DownloadSnapshot> {
    return mutate("download_speed_limit_set", { jobId, bytesPerSecond });
  }
  createQueue(name: string, maxActive?: number): Promise<QueueInfo> {
    const args: Record<string, unknown> = { name };
    if (maxActive !== undefined) args.maxActive = maxActive;
    return mutate<QueueInfo>("queues_create", args);
  }
  renameQueue(queueId: string, name: string): Promise<QueueInfo> {
    return mutate<QueueInfo>("queues_rename", { queueId, name });
  }
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo> {
    return mutate<QueueInfo>("queues_set_limit", { queueId, maxActive });
  }
  setQueueSchedule(queueId: string, schedule: QueueSchedule | null): Promise<QueueInfo> {
    return mutate<QueueInfo>("queues_set_schedule", { queueId, schedule });
  }
  async deleteQueue(queueId: string, moveTo?: string): Promise<void> {
    const args: Record<string, unknown> = { queueId };
    if (moveTo !== undefined) args.moveTo = moveTo;
    await mutate<unknown>("queues_delete", args);
  }
  startQueue(queueId: string): Promise<QueueInfo> {
    return mutate<QueueInfo>("queues_start", { queueId });
  }
  stopQueue(queueId: string): Promise<QueueInfo> {
    return mutate<QueueInfo>("queues_stop", { queueId });
  }
  getHistory(): Promise<HistorySettings> {
    return read<HistorySettings>("history_get");
  }
  setHistory(retentionDays: number | null): Promise<HistorySettings> {
    return mutate<HistorySettings>("history_set", { retentionDays });
  }
  previewHistory(retentionDays: number | null): Promise<HistoryPreview> {
    return read<HistoryPreview>("history_preview", { retentionDays });
  }
  cleanHistory(): Promise<HistoryCleanResult> {
    return mutate<HistoryCleanResult>("history_clean", {});
  }
  retryDownload(
    jobId: JobId,
    operationId: string,
    options: RetryOptions,
  ): Promise<DownloadSnapshot> {
    return mutate("downloads_retry", {
      jobId,
      operationId,
      expectedAttemptId: options.expectedAttemptId,
      discardPartial: options.discardPartial,
    });
  }
  replaceSource(
    jobId: JobId,
    operationId: string,
    url: string,
    mode: ReplaceMode,
    expectedAttemptId: string | null,
  ): Promise<ReplaceSourceResult> {
    return mutate<ReplaceSourceResult>("downloads_replace_source", {
      jobId,
      operationId,
      url,
      mode,
      expectedAttemptId,
    });
  }
  revealDownload(jobId: JobId): Promise<Revealed> {
    // Opening a folder changes no state: a failure is never "uncertain".
    return read<Revealed>("downloads_reveal", { jobId });
  }
  getNotifications(): Promise<NotificationStatus> {
    return read<NotificationStatus>("notifications_get");
  }
  setNotifications(enabled: boolean, locale: NotificationLocale): Promise<NotificationStatus> {
    return mutate<NotificationStatus>("notifications_set", { enabled, locale });
  }
}

async function mutate<T = DownloadSnapshot>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw toCallError(e, true);
  }
}

async function read<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await (args === undefined ? invoke<T>(command) : invoke<T>(command, args));
  } catch (e) {
    throw toCallError(e);
  }
}

/** Window events shared with the shell (`stream::SERVICE_EVENT`, `stream::STREAM_ENDED`). */
export const SERVICE_EVENT = "downova://service-event";
export const STREAM_ENDED = "downova://service-stream-ended";

export function createServiceClient(): ServiceClient {
  // In a plain browser (`pnpm dev`) there is no shell and no service bridge.
  return isTauri() ? new TauriServiceClient() : new FoundationServiceClient();
}

// ------------------------------------------------------------------ desktop shell (B2.2)

export type CloseBehaviour = "hide_to_tray" | "close_window";

export interface DesktopSettings {
  close_behaviour: CloseBehaviour;
  /** False when the tray could not be created: closing then minimizes instead of hiding. */
  tray_available: boolean;
}

export interface ExitReport {
  paused_jobs: number;
  /** The service was verified gone (not just acknowledged). The desktop then quits. */
  stopped: boolean;
}

/** The desktop window's own lifecycle: close behaviour, tray labels and "Exit and stop downloads". */

// ------------------------------------------------------------------ updates (ADV09, ADR 0030)

/** Why this installation has no in-app update. */
export type UpdateNotConfigured = "missing" | "invalid" | "package_manager" | "unpackaged";

/** The shell's update state (`update::UpdateView`). One operation at a time. */
export type UpdateView =
  | { state: "not_configured"; reason: UpdateNotConfigured }
  | { state: "idle" }
  | { state: "checking"; operation_id: string }
  | { state: "no_update"; operation_id: string }
  | {
      state: "available";
      operation_id: string;
      version: string;
      date: string | null;
      notes: string | null;
    }
  | {
      state: "downloading";
      operation_id: string;
      version: string;
      downloaded: number;
      total: number | null;
    }
  | { state: "verifying"; operation_id: string; version: string }
  | { state: "verified"; operation_id: string; version: string }
  | { state: "preparing"; operation_id: string; version: string }
  | {
      state: "ready_to_install";
      operation_id: string;
      version: string;
      prepared: PrepareUpdateResult;
    }
  | { state: "installing"; operation_id: string; version: string }
  | { state: "recovery_required"; kind: string; backup_operation: string | null }
  | {
      state: "failed";
      operation_id: string | null;
      stage: "check" | "download" | "verify" | "prepare" | "shutdown" | "install" | "restore";
      error: ErrorData;
      can_retry_prepare: boolean;
    };

export interface UpdateStatus {
  current_version: string;
  /** This build's package format (nsis, msi, appimage, app), when it has one. */
  package: string | null;
  /** A test binary built with the update-fixture feature (never a normal package). */
  fixture: boolean;
  view: UpdateView;
}

/** Fixed update steps of the shell (no URL, key, path or installer ever comes from the page). */
export interface UpdateShell {
  updateStatus(): Promise<UpdateStatus>;
  updateCheck(): Promise<UpdateView>;
  updateDownload(operationId: string): Promise<UpdateView>;
  updatePrepare(operationId: string): Promise<UpdateView>;
  updateCancel(operationId: string): Promise<UpdateView>;
  /** Only after the user confirmed installing. */
  updateInstall(operationId: string): Promise<UpdateView>;
  /** Recovery required only: restores the recorded, checked pre-update backup. */
  updateRestore(operationId: string): Promise<UpdateView>;
  /** Every state change of the shell. Returns an unsubscribe. */
  onUpdate(handler: (view: UpdateView) => void): () => void;
}

/** Event name shared with the shell (`update::UPDATE_EVENT`). */
export const UPDATE_EVENT = "downova://update";

export interface DesktopShell extends UpdateShell {
  /** False in the plain browser: there is no window lifecycle to control. */
  readonly available: boolean;
  getSettings(): Promise<DesktopSettings | null>;
  setCloseBehaviour(behaviour: CloseBehaviour): Promise<DesktopSettings>;
  setTrayLabels(open: string, exit: string): Promise<void>;
  /** Only after the user confirmed. Rejects with a ServiceCallError; never retried automatically. */
  exitAndStop(): Promise<ExitReport>;
  /** The tray's "Exit and stop downloads" asks the window to confirm. Returns an unsubscribe. */
  onExitRequested(handler: () => void): () => void;
  /**
   * Browse… (B2.19): the native folder dialog after a user's click. Resolves with the chosen
   * folder, or null when the user cancelled (nothing changes then).
   */
  pickFolder(start: string | null, title: string): Promise<string | null>;
  /**
   * Add batch download from clipboard: the clipboard's text, read by the desktop on the user's
   * command (null when it holds no text). Rejects when there is no desktop.
   */
  clipboardText(): Promise<string | null>;
  /** F08: which finish actions this system offers (none without a desktop). */
  powerSupport(): Promise<PowerSupport>;
  /** F08: runs the action after the window's countdown; false when it could not start. */
  powerRun(action: PowerAction): Promise<boolean>;
  /** Brings the window forward (the F08 countdown must be seen). */
  showWindow(): Promise<void>;
}

/** F08 finish actions. */
export type PowerAction = "sleep" | "shutdown";
export interface PowerSupport {
  sleep: boolean;
  shutdown: boolean;
}

export class BrowserShell implements DesktopShell {
  readonly available = false;
  async getSettings(): Promise<DesktopSettings | null> {
    return null;
  }
  setCloseBehaviour(): Promise<DesktopSettings> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  async setTrayLabels(): Promise<void> {}
  exitAndStop(): Promise<ExitReport> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  onExitRequested(): () => void {
    return () => {};
  }
  pickFolder(): Promise<string | null> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  clipboardText(): Promise<string | null> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  async powerSupport(): Promise<PowerSupport> {
    return { sleep: false, shutdown: false };
  }
  async powerRun(): Promise<boolean> {
    return false;
  }
  async showWindow(): Promise<void> {}
  async updateStatus(): Promise<UpdateStatus> {
    return {
      current_version: "",
      package: null,
      fixture: false,
      view: { state: "not_configured", reason: "unpackaged" },
    };
  }
  private noUpdates(): Promise<never> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  updateCheck = () => this.noUpdates();
  updateDownload = () => this.noUpdates();
  updatePrepare = () => this.noUpdates();
  updateCancel = () => this.noUpdates();
  updateInstall = () => this.noUpdates();
  updateRestore = () => this.noUpdates();
  onUpdate(): () => void {
    return () => {};
  }
}

export class TauriShell implements DesktopShell {
  readonly available = true;
  async getSettings(): Promise<DesktopSettings | null> {
    try {
      return await invoke<DesktopSettings>("desktop_settings_get");
    } catch {
      return null;
    }
  }
  async setCloseBehaviour(behaviour: CloseBehaviour): Promise<DesktopSettings> {
    try {
      return await invoke<DesktopSettings>("desktop_settings_set", { closeBehaviour: behaviour });
    } catch (e) {
      throw toCallError(e);
    }
  }
  async setTrayLabels(open: string, exit: string): Promise<void> {
    try {
      await invoke("tray_set_labels", { open, exit });
    } catch {
      // English labels stay; nothing else depends on them.
    }
  }
  async exitAndStop(): Promise<ExitReport> {
    try {
      return await invoke<ExitReport>("service_exit_and_stop");
    } catch (e) {
      throw toCallError(e, true);
    }
  }
  onExitRequested(handler: () => void): () => void {
    let stopped = false;
    let unlisten: (() => void) | undefined;
    void listen(EXIT_REQUESTED, () => handler()).then((u) => {
      if (stopped) u();
      else unlisten = u;
    });
    return () => {
      stopped = true;
      unlisten?.();
    };
  }
  async pickFolder(start: string | null, title: string): Promise<string | null> {
    try {
      return await invoke<string | null>("folder_pick", { start, title });
    } catch (e) {
      throw toCallError(e);
    }
  }
  async clipboardText(): Promise<string | null> {
    try {
      return await invoke<string | null>("clipboard_text");
    } catch (e) {
      throw toCallError(e);
    }
  }
  async powerSupport(): Promise<PowerSupport> {
    try {
      return await invoke<PowerSupport>("power_support");
    } catch {
      return { sleep: false, shutdown: false };
    }
  }
  async powerRun(action: PowerAction): Promise<boolean> {
    try {
      return await invoke<boolean>("power_run", { action });
    } catch {
      return false;
    }
  }
  async showWindow(): Promise<void> {
    try {
      await invoke("window_show");
    } catch {
      // Best effort: the countdown is still shown in the window.
    }
  }
  async updateStatus(): Promise<UpdateStatus> {
    try {
      return await invoke<UpdateStatus>("update_status");
    } catch (e) {
      throw toCallError(e);
    }
  }
  private async step(command: string, args: Record<string, string> = {}): Promise<UpdateView> {
    try {
      return await invoke<UpdateView>(command, args);
    } catch (e) {
      throw toCallError(e);
    }
  }
  updateCheck = () => this.step("update_check");
  updateDownload = (operationId: string) => this.step("update_download", { operationId });
  updatePrepare = (operationId: string) => this.step("update_prepare", { operationId });
  updateCancel = (operationId: string) => this.step("update_cancel", { operationId });
  updateInstall = (operationId: string) => this.step("update_install", { operationId });
  updateRestore = (operationId: string) => this.step("update_restore", { operationId });
  onUpdate(handler: (view: UpdateView) => void): () => void {
    let stopped = false;
    let unlisten: (() => void) | undefined;
    void listen<UpdateView>(UPDATE_EVENT, (e) => handler(e.payload)).then((u) => {
      if (stopped) u();
      else unlisten = u;
    });
    return () => {
      stopped = true;
      unlisten?.();
    };
  }
}

/** Event name shared with the shell (`lifecycle::EXIT_REQUESTED`). */
export const EXIT_REQUESTED = "downova://exit-requested";

export function createDesktopShell(): DesktopShell {
  return isTauri() ? new TauriShell() : new BrowserShell();
}
