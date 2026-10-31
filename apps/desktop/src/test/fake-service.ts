// Test double for the service-client boundary. It records every call and lets a test decide how
// each one ends, including answers that arrive late (stale polling) or never (uncertain adds).
import type {
  DownloadSnapshot,
  DuplicateSummary,
  ErrorData,
  EventKind,
  HistoryCleanResult,
  HistoryPreview,
  HistorySettings,
  HelloResult,
  JobId,
  MoveTarget,
  Priority,
  QueueInfo,
  ServiceEvent,
  ServiceSettings,
} from "@dm/contracts";
import {
  ServiceCallError,
  type CallOutcome,
  type CloseBehaviour,
  type ConnectionState,
  type DesktopSettings,
  type DesktopShell,
  type ExitReport,
  type AddOptions,
  type ServiceClient,
  type StreamEnd,
  type StreamHandlers,
  type Subscription,
} from "../services/service-client";

export const HELLO = {
  api_version: 1,
  service_version: "0.1.0",
  service_instance_id: "svc_test",
  backend: { id: "http", version: "0.1.0", capabilities: [] },
} as unknown as HelloResult;

export function snapshot(overrides: Partial<DownloadSnapshot> = {}): DownloadSnapshot {
  return {
    job_id: "job_1",
    attempt_id: null,
    display_name: "file.zip",
    source_display: "https://example.org/file.zip",
    desired_state: "running",
    phase: "transferring",
    blockers: [],
    downloaded_bytes: "0",
    total_bytes: null,
    error: null,
    ...overrides,
  };
}

export function callError(
  outcome: CallOutcome,
  kind: ErrorData["kind"] = "SERVICE_UNAVAILABLE",
  next_action: ErrorData["next_action"] = "retry",
): ServiceCallError {
  return new ServiceCallError(outcome, { kind, retryable: false, next_action, detail: null });
}

type Handler<T> = () => Promise<T>;

export interface Call {
  method:
    | "connect"
    | "probe"
    | "subscribe"
    | "list"
    | "add"
    | "pause"
    | "resume"
    | "cancel"
    | "remove"
    | "check"
    | "confirmName"
    | "setQueue"
    | "setPriority"
    | "move"
    | "settingsGet"
    | "settingsSet"
    | "queues"
    | "queueCreate"
    | "queueRename"
    | "queueLimit"
    | "queueDelete"
    | "queueStart"
    | "queueStop"
    | "historyGet"
    | "historySet"
    | "historyPreview"
    | "historyClean";
  args: string[];
}

/**
 * A scriptable ServiceClient. By default it is connected, lists `items`, and acknowledges
 * mutations by updating `items` like the service would. Queue a handler to override one call.
 */
export class FakeServiceClient implements ServiceClient {
  items: DownloadSnapshot[] = [];
  calls: Call[] = [];
  connection: ConnectionState = { status: "connected", hello: HELLO };
  private queued: Partial<Record<Call["method"], Handler<unknown>[]>> = {};
  private nextJob = 1;

  /** The next call of `method` ends as `handler` says (a never-settling promise is allowed). */
  next<T>(method: Call["method"], handler: Handler<T>): this {
    (this.queued[method] ??= []).push(handler as Handler<unknown>);
    return this;
  }

  count(method: Call["method"]): number {
    return this.calls.filter((c) => c.method === method).length;
  }

  private run<T>(method: Call["method"], args: string[], fallback: Handler<T>): Promise<T> {
    this.calls.push({ method, args });
    const handler = this.queued[method]?.shift() as Handler<T> | undefined;
    return (handler ?? fallback)();
  }

  connect(): Promise<ConnectionState> {
    return this.run("connect", [], async () => this.connection);
  }

  listDownloads(): Promise<DownloadSnapshot[]> {
    return this.run("list", [], async () => {
      if (this.connection.status !== "connected") throw callError("unavailable");
      return this.items.map((d) => ({ ...d }));
    });
  }

  addDownload(url: string, key: string, options?: AddOptions): Promise<DownloadSnapshot> {
    this.lastAddOptions = options;
    return this.run("add", [url, key], async () => {
      const job = snapshot({
        job_id: `job_new${this.nextJob++}`,
        display_name: url.split("/").pop() || "download",
        source_display: url,
        phase: "queued",
      });
      this.items = [...this.items, job];
      return job;
    });
  }

  private control(method: "pause" | "resume" | "cancel", jobId: JobId) {
    return this.run(method, [jobId], async () => {
      const job = this.items.find((d) => d.job_id === jobId);
      if (!job) throw callError("rejected", "INVALID_REQUEST", "none");
      const next: DownloadSnapshot =
        method === "pause"
          ? { ...job, desired_state: "paused" }
          : method === "resume"
            ? { ...job, desired_state: "running" }
            : { ...job, desired_state: "cancelled", phase: "cancelled" };
      this.items = this.items.map((d) => (d.job_id === jobId ? next : d));
      return next;
    });
  }

  pauseDownload(jobId: JobId) {
    return this.control("pause", jobId);
  }
  resumeDownload(jobId: JobId) {
    return this.control("resume", jobId);
  }
  cancelDownload(jobId: JobId) {
    return this.control("cancel", jobId);
  }
  /** Like the service: only finished jobs; the job leaves later lists but keeps answering. */
  removeDownload(jobId: JobId) {
    return this.run("remove", [jobId], async () => {
      const job = this.items.find((d) => d.job_id === jobId);
      if (!job || !["completed", "failed", "cancelled"].includes(job.phase)) {
        throw callError("rejected", "INVALID_REQUEST", "none");
      }
      this.items = this.items.filter((d) => d.job_id !== jobId);
      return { ...job, removed: true };
    });
  }

  // ---------------------------------------------------------------- B2.8–B2.11 additions

  /** Streaming is opt-in: by default the fake refuses subscriptions like a pre-B2.9 service. */
  streaming = false;
  instance = "svc_test";
  sequence = 0n;
  private handlers: StreamHandlers | null = null;
  subscriptions = 0;
  closedSubscriptions = 0;
  /** What probe() answers (attach-only reconnection). */
  probeAnswer: ConnectionState | null = null;
  settings: ServiceSettings = { destination_rules: [], on_collision: "rename" };
  queues: QueueInfo[] = [
    { queue_id: "main", name: "Main queue", position: 0, max_active: 3, running: true },
  ];
  /** Same-link summaries by URL for check(). */
  duplicates = new Map<string, DuplicateSummary[]>();
  /** Last options passed to addDownload. */
  lastAddOptions: AddOptions | undefined;
  private nextQueue = 1;

  probe(): Promise<ConnectionState> {
    return this.run("probe", [], async () => this.probeAnswer ?? this.connection);
  }

  subscribe(handlers: StreamHandlers): Promise<Subscription> {
    return this.run("subscribe", [], async () => {
      if (this.connection.status !== "connected") throw callError("unavailable");
      if (!this.streaming) {
        throw callError("rejected", "INVALID_REQUEST", "none");
      }
      this.subscriptions += 1;
      this.handlers = handlers;
      const mine = handlers;
      return {
        start: {
          service_instance_id: this.instance,
          sequence: this.sequence.toString(),
          items: this.items.map((d) => ({ ...d })),
        },
        close: () => {
          this.closedSubscriptions += 1;
          if (this.handlers === mine) this.handlers = null;
        },
      };
    });
  }

  /** Sends one event to the current subscriber with the next sequence (or an explicit one). */
  emit(kind: EventKind, job: DownloadSnapshot | null, sequence?: bigint): ServiceEvent {
    this.sequence = sequence ?? this.sequence + 1n;
    const event: ServiceEvent = {
      service_instance_id: this.instance,
      sequence: this.sequence.toString(),
      kind,
      job_id: job?.job_id ?? null,
      attempt_id: job?.attempt_id ?? null,
      snapshot: job,
    };
    this.handlers?.onEvent(event);
    return event;
  }

  /** Replaces a job like the service would and announces it. */
  change(job: DownloadSnapshot, kind: EventKind = "job_changed"): void {
    this.items = this.items.some((d) => d.job_id === job.job_id)
      ? this.items.map((d) => (d.job_id === job.job_id ? job : d))
      : [...this.items, job];
    this.emit(kind, job);
  }

  endStream(reason: StreamEnd): void {
    const h = this.handlers;
    this.handlers = null;
    h?.onEnded(reason);
  }

  get subscribed(): boolean {
    return this.handlers !== null;
  }

  checkLink(url: string): Promise<DuplicateSummary[]> {
    return this.run("check", [url], async () => this.duplicates.get(url) ?? []);
  }

  private update(
    method: Call["method"],
    jobId: JobId,
    args: string[],
    change: (job: DownloadSnapshot) => DownloadSnapshot,
  ): Promise<DownloadSnapshot> {
    return this.run(method, [jobId, ...args], async () => {
      const job = this.items.find((d) => d.job_id === jobId);
      if (!job) throw callError("rejected", "INVALID_REQUEST", "none");
      const next = change(job);
      this.items = this.items.map((d) => (d.job_id === jobId ? next : d));
      return next;
    });
  }

  confirmName(jobId: JobId) {
    return this.update("confirmName", jobId, [], (j) => ({
      ...j,
      blockers: j.blockers.filter((b) => b !== "name_conflict"),
    }));
  }
  setJobQueue(jobId: JobId, queueId: string) {
    return this.update("setQueue", jobId, [queueId], (j) => ({ ...j, queue_id: queueId }));
  }
  setPriority(jobId: JobId, priority: Priority) {
    return this.update("setPriority", jobId, [priority], (j) => ({ ...j, priority }));
  }
  moveJob(jobId: JobId, to: MoveTarget) {
    return this.update("move", jobId, [to], (j) => ({ ...j }));
  }
  getSettings(): Promise<ServiceSettings> {
    return this.run("settingsGet", [], async () => structuredClone(this.settings));
  }
  setSettings(settings: ServiceSettings): Promise<ServiceSettings> {
    return this.run("settingsSet", [JSON.stringify(settings)], async () => {
      this.settings = structuredClone(settings);
      return structuredClone(settings);
    });
  }
  listQueues(): Promise<QueueInfo[]> {
    return this.run("queues", [], async () => this.queues.map((q) => ({ ...q })));
  }
  private queue(id: string): QueueInfo {
    const q = this.queues.find((x) => x.queue_id === id);
    if (!q) throw callError("rejected", "INVALID_REQUEST", "none");
    return q;
  }
  createQueue(name: string, maxActive = 3): Promise<QueueInfo> {
    return this.run("queueCreate", [name], async () => {
      const q: QueueInfo = {
        queue_id: `q_${this.nextQueue++}`,
        name,
        position: this.queues.length,
        max_active: maxActive,
        running: true,
      };
      this.queues = [...this.queues, q];
      return { ...q };
    });
  }
  renameQueue(queueId: string, name: string): Promise<QueueInfo> {
    return this.run("queueRename", [queueId, name], async () => {
      const q = { ...this.queue(queueId), name };
      this.queues = this.queues.map((x) => (x.queue_id === queueId ? q : x));
      return q;
    });
  }
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo> {
    return this.run("queueLimit", [queueId, String(maxActive)], async () => {
      const q = { ...this.queue(queueId), max_active: maxActive };
      this.queues = this.queues.map((x) => (x.queue_id === queueId ? q : x));
      return q;
    });
  }
  /** Like the service: 'main' cannot go, and a queue with jobs needs a target. */
  deleteQueue(queueId: string, moveTo?: string): Promise<void> {
    return this.run("queueDelete", moveTo ? [queueId, moveTo] : [queueId], async () => {
      this.queue(queueId);
      const members = this.items.filter((d) => (d.queue_id ?? "main") === queueId);
      if (queueId === "main" || (members.length > 0 && !moveTo)) {
        throw callError("rejected", "INVALID_REQUEST", "none");
      }
      if (moveTo) {
        this.queue(moveTo);
        this.items = this.items.map((d) =>
          (d.queue_id ?? "main") === queueId ? { ...d, queue_id: moveTo } : d,
        );
      }
      this.queues = this.queues.filter((x) => x.queue_id !== queueId);
    });
  }
  private setRunning(method: "queueStart" | "queueStop", queueId: string, running: boolean) {
    return this.run(method, [queueId], async () => {
      const q = { ...this.queue(queueId), running };
      this.queues = this.queues.map((x) => (x.queue_id === queueId ? q : x));
      return q;
    });
  }
  startQueue(queueId: string): Promise<QueueInfo> {
    return this.setRunning("queueStart", queueId, true);
  }
  stopQueue(queueId: string): Promise<QueueInfo> {
    return this.setRunning("queueStop", queueId, false);
  }

  /** Retention like the service: finished jobs listed in `expired` leave the list on clean. */
  retentionDays: number | null = null;
  /** Job IDs a policy would remove now (the test decides; the fake has no clock). */
  expired = new Set<string>();
  private finishedListed(): DownloadSnapshot[] {
    return this.items.filter((d) => ["completed", "failed", "cancelled"].includes(d.phase));
  }
  getHistory(): Promise<HistorySettings> {
    return this.run("historyGet", [], async () => ({ retention_days: this.retentionDays }));
  }
  setHistory(retentionDays: number | null): Promise<HistorySettings> {
    return this.run("historySet", [String(retentionDays)], async () => {
      this.retentionDays = retentionDays;
      return { retention_days: retentionDays };
    });
  }
  previewHistory(retentionDays: number | null): Promise<HistoryPreview> {
    return this.run("historyPreview", [String(retentionDays)], async () => {
      const finished = this.finishedListed();
      return {
        expiring:
          retentionDays === null ? 0 : finished.filter((d) => this.expired.has(d.job_id)).length,
        finished: finished.length,
      };
    });
  }
  cleanHistory(): Promise<HistoryCleanResult> {
    return this.run("historyClean", [], async () => {
      if (this.retentionDays === null) return { removed: 0, kept: 0 };
      const gone = this.finishedListed().filter((d) => this.expired.has(d.job_id));
      this.items = this.items.filter((d) => !gone.some((g) => g.job_id === d.job_id));
      for (const job of gone) this.emit("job_removed", { ...job, removed: true });
      return { removed: gone.length, kept: 0 };
    });
  }
}

/** A promise the test resolves or rejects later. */
export function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Test double for the desktop shell (B2.2). */
export class FakeShell implements DesktopShell {
  readonly available = true;
  settings: DesktopSettings = { close_behaviour: "hide_to_tray", tray_available: true };
  labels: [string, string][] = [];
  saves: CloseBehaviour[] = [];
  exits = 0;
  failSave = false;
  private exitHandlers = new Set<() => void>();
  private queuedExit: (() => Promise<ExitReport>)[] = [];

  async getSettings(): Promise<DesktopSettings | null> {
    return { ...this.settings };
  }
  async setCloseBehaviour(behaviour: CloseBehaviour): Promise<DesktopSettings> {
    this.saves.push(behaviour);
    if (this.failSave) throw callError("rejected", "INTERNAL", "none");
    this.settings = { ...this.settings, close_behaviour: behaviour };
    return { ...this.settings };
  }
  async setTrayLabels(open: string, exit: string): Promise<void> {
    this.labels.push([open, exit]);
  }
  /** The next exitAndStop ends as `handler` says. */
  nextExit(handler: () => Promise<ExitReport>): this {
    this.queuedExit.push(handler);
    return this;
  }
  exitAndStop(): Promise<ExitReport> {
    this.exits += 1;
    const handler = this.queuedExit.shift();
    return handler ? handler() : Promise.resolve({ paused_jobs: 0, stopped: true });
  }
  onExitRequested(handler: () => void): () => void {
    this.exitHandlers.add(handler);
    return () => this.exitHandlers.delete(handler);
  }
  /** What the tray's "Exit and stop downloads" does. */
  requestExit(): void {
    for (const h of this.exitHandlers) h();
  }
}
