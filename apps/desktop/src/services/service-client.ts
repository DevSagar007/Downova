// Service-client boundary: the ONLY module family in the UI allowed to reach the desktop shell
// (Tauri) and, through it, the background service. Features depend on the `ServiceClient`
// interface, never on transport details. Contract types come from @dm/contracts.
import type {
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
  ListQueuesResult,
  MoveTarget,
  Priority,
  QueueInfo,
  ServiceEvent,
  ServiceSettings,
  SubscribeResult,
} from "@dm/contracts";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type ConnectionState =
  | { status: "connected"; hello: HelloResult }
  | {
      status: "unavailable";
      /** Why the UI cannot talk to the service. Presented to the user honestly. */
      reason: "not_implemented" | "not_running" | "incompatible_version";
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
}

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
  removeDownload(jobId: JobId): Promise<DownloadSnapshot>;
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
  listQueues(): Promise<QueueInfo[]>;
  createQueue(name: string, maxActive?: number): Promise<QueueInfo>;
  renameQueue(queueId: string, name: string): Promise<QueueInfo>;
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo>;
  /** Never deletes jobs or files: a queue that still has jobs needs `moveTo`. */
  deleteQueue(queueId: string, moveTo?: string): Promise<void>;
  startQueue(queueId: string): Promise<QueueInfo>;
  stopQueue(queueId: string): Promise<QueueInfo>;
  /** history.v1.* (B2.12): retention of finished list entries; files are never touched. */
  getHistory(): Promise<HistorySettings>;
  setHistory(retentionDays: number | null): Promise<HistorySettings>;
  previewHistory(retentionDays: number | null): Promise<HistoryPreview>;
  cleanHistory(): Promise<HistoryCleanResult>;
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
  listQueues = (): Promise<QueueInfo[]> => this.refuse();
  createQueue = (): Promise<QueueInfo> => this.refuse();
  renameQueue = (): Promise<QueueInfo> => this.refuse();
  setQueueLimit = (): Promise<QueueInfo> => this.refuse();
  deleteQueue = (): Promise<void> => this.refuse();
  startQueue = (): Promise<QueueInfo> => this.refuse();
  stopQueue = (): Promise<QueueInfo> => this.refuse();
  getHistory = (): Promise<HistorySettings> => this.refuse();
  setHistory = (): Promise<HistorySettings> => this.refuse();
  previewHistory = (): Promise<HistoryPreview> => this.refuse();
  cleanHistory = (): Promise<HistoryCleanResult> => this.refuse();
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
  removeDownload(jobId: JobId): Promise<DownloadSnapshot> {
    return mutate("downloads_remove", { jobId });
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
export interface DesktopShell {
  /** False in the plain browser: there is no window lifecycle to control. */
  readonly available: boolean;
  getSettings(): Promise<DesktopSettings | null>;
  setCloseBehaviour(behaviour: CloseBehaviour): Promise<DesktopSettings>;
  setTrayLabels(open: string, exit: string): Promise<void>;
  /** Only after the user confirmed. Rejects with a ServiceCallError; never retried automatically. */
  exitAndStop(): Promise<ExitReport>;
  /** The tray's "Exit and stop downloads" asks the window to confirm. Returns an unsubscribe. */
  onExitRequested(handler: () => void): () => void;
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
}

/** Event name shared with the shell (`lifecycle::EXIT_REQUESTED`). */
export const EXIT_REQUESTED = "downova://exit-requested";

export function createDesktopShell(): DesktopShell {
  return isTauri() ? new TauriShell() : new BrowserShell();
}
