import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type {
  DownloadSnapshot,
  JobId,
  MoveTarget,
  Priority,
  QueueInfo,
  QueueSchedule,
  ServiceEvent,
  ServiceSettings,
} from "@dm/contracts";
import {
  BrowserShell,
  createDesktopShell,
  createServiceClient,
  type AddOptions,
  type DesktopShell,
  toCallError,
  type ConnectionState,
  type ServiceCallError,
  type ServiceClient,
  type Subscription,
} from "./service-client";

type ServiceState = { status: "checking" } | ConnectionState;

/** How often the list is refreshed in the polling fallback (a service without events). */
export const POLL_INTERVAL_MS = 1000;

/**
 * Automatic reconnection after the service went away (B2.10): attach-only probes (they never
 * start the service) at these delays, then it stops and waits for the user's Reconnect.
 */
export const RECONNECT_DELAYS_MS: readonly number[] = [500, 1000, 2000, 4000, 8000, 8000];

/**
 * Resubscription after a stream ended, a gap or another instance (B2.24): the first
 * `RESUBSCRIBE_FREE` in a row are immediate; further ones, while no event has been applied in
 * between, wait `RESUBSCRIBE_BACKOFF_MS` (doubling, capped). A service that keeps ending streams
 * can therefore never drive a tight subscribe-and-snapshot loop.
 */
export const RESUBSCRIBE_FREE = 3;
export const RESUBSCRIBE_BACKOFF_MS = 500;
export const RESUBSCRIBE_MAX_MS = 8_000;

/** How live updates arrive: the event stream, or polling when the service offers no events. */
export type LiveMode = "stream" | "polling" | "none";

export interface DownloadsState {
  /** The service's snapshots, in service order. Kept (and marked stale) when disconnected. */
  items: DownloadSnapshot[];
  /** At least one list has arrived since the app started. */
  loaded: boolean;
  /** The items may be out of date: the last update failed or the service is unreachable. */
  stale: boolean;
  /** The latest job seen moving into completed, failed or cancelled (for a polite announcement). */
  ended: Ended | null;
  /** The service's queues in list order (empty until known). */
  queues: QueueInfo[];
  /** How updates currently arrive. */
  mode: LiveMode;
  /** Automatic reconnection is under way. */
  reconnecting: boolean;
}

export interface Ended {
  jobId: string;
  name: string;
  phase: "completed" | "failed" | "cancelled";
  /** Increases with every new transition, so the same text can be announced twice. */
  seq: number;
}

const TERMINAL = ["completed", "failed", "cancelled"] as const;

/** A job of `before` that is now newly finished, if any (none on the first list). */
function newlyEnded(before: DownloadsState, items: DownloadSnapshot[]): Omit<Ended, "seq"> | null {
  if (!before.loaded) return null;
  const was = new Map(before.items.map((d) => [d.job_id, d.phase]));
  for (const d of items) {
    const prev = was.get(d.job_id);
    const phase = TERMINAL.find((p) => p === d.phase);
    if (prev !== undefined && prev !== d.phase && phase) {
      return { jobId: d.job_id, name: d.display_name, phase };
    }
  }
  return null;
}

/** `d` with `items` replacing its list (and a newly ended job noted). */
function withItems(d: DownloadsState, items: DownloadSnapshot[]): DownloadsState {
  const ended = newlyEnded(d, items);
  return {
    ...d,
    items,
    loaded: true,
    stale: false,
    ended: ended ? { ...ended, seq: (d.ended?.seq ?? 0) + 1 } : d.ended,
  };
}

/**
 * Event batching (ADR 0022 performance notes): events are kept per job — the latest snapshot or a
 * removal — and applied together. A batch is therefore bounded by the number of jobs, never by
 * how long the window was hidden. Visible windows apply within `VISIBLE_FLUSH_MS` (well inside
 * the service's 400 ms progress coalescing); a hidden window applies at most once per
 * `HIDDEN_FLUSH_MS` and at once when it becomes visible again.
 */
export const VISIBLE_FLUSH_MS = 50;
export const HIDDEN_FLUSH_MS = 1_000;

/** Applies a batch (job ID → snapshot, or null for a removal) with one pass over the list. */
function applyBatch(
  d: DownloadsState,
  batch: Map<string, DownloadSnapshot | null>,
): DownloadsState {
  const index = new Map(d.items.map((x, i) => [x.job_id, i]));
  let items: (DownloadSnapshot | null)[] = d.items;
  let copied = false;
  const copy = () => {
    if (!copied) {
      items = [...items];
      copied = true;
    }
  };
  const appended: DownloadSnapshot[] = [];
  let ended: Omit<Ended, "seq"> | null = null;
  for (const [id, job] of batch) {
    const at = index.get(id);
    if (job === null || job.removed) {
      if (at !== undefined) {
        copy();
        items[at] = null;
      }
      continue;
    }
    const before = at === undefined ? undefined : d.items[at];
    const phase = TERMINAL.find((p) => p === job.phase);
    if (d.loaded && before && before.phase !== job.phase && phase) {
      ended = { jobId: job.job_id, name: job.display_name, phase };
    }
    if (at === undefined) appended.push(job);
    else {
      copy();
      items[at] = job;
    }
  }
  const next = [...(items.filter((x) => x !== null) as DownloadSnapshot[]), ...appended];
  return {
    ...d,
    items: next,
    loaded: true,
    stale: false,
    ended: ended ? { ...ended, seq: (d.ended?.seq ?? 0) + 1 } : d.ended,
  };
}

/** Inserts or replaces one job, keeping service order; removed jobs leave the list. */
function upsert(items: DownloadSnapshot[], job: DownloadSnapshot): DownloadSnapshot[] {
  if (job.removed) return items.filter((x) => x.job_id !== job.job_id);
  return items.some((x) => x.job_id === job.job_id)
    ? items.map((x) => (x.job_id === job.job_id ? job : x))
    : [...items, job];
}

export interface ServiceActions {
  /** Connects again (the desktop shell finds or starts the service). Only on the user's request. */
  reconnect(): void;
  /** Ends live updates and automatic reconnection before "Exit and stop downloads" (B2.10). */
  beginExit(): void;
  /** The exit did not stop the service: live updates resume. */
  cancelExit(): void;
  /** Each resolves with the service's acknowledgement or rejects with a ServiceCallError. They
   * are never repeated automatically. */
  add(url: string, idempotencyKey: string, options?: AddOptions): Promise<DownloadSnapshot>;
  pause(jobId: JobId): Promise<DownloadSnapshot>;
  resume(jobId: JobId): Promise<DownloadSnapshot>;
  cancel(jobId: JobId): Promise<DownloadSnapshot>;
  remove(jobId: JobId): Promise<DownloadSnapshot>;
  confirmName(jobId: JobId): Promise<DownloadSnapshot>;
  setJobQueue(jobId: JobId, queueId: string): Promise<DownloadSnapshot>;
  setPriority(jobId: JobId, priority: Priority): Promise<DownloadSnapshot>;
  moveJob(jobId: JobId, to: MoveTarget): Promise<DownloadSnapshot>;
  /** Reads that do not change state; they reject with a ServiceCallError. */
  checkLink: ServiceClient["checkLink"];
  getSettings(): Promise<ServiceSettings>;
  setSettings(settings: ServiceSettings): Promise<ServiceSettings>;
  getConcurrency: ServiceClient["getConcurrency"];
  /** Reads the queues again from the service (their limits feed the Options explanation). */
  reloadQueues(): Promise<void>;
  setConcurrency: ServiceClient["setConcurrency"];
  /** CORE-06 (reads and saves reject with a ServiceCallError). */
  /** BRW-07/09 (rejects with a ServiceCallError). */
  browserConnections: ServiceClient["browserConnections"];
  repairBrowser: ServiceClient["repairBrowser"];
  /** SEC-02 (rejects with a ServiceCallError). */
  listCredentials: ServiceClient["listCredentials"];
  forgetCredential: ServiceClient["forgetCredential"];
  /** E05 (reads and exports reject with a ServiceCallError). */
  previewDiagnostics: ServiceClient["previewDiagnostics"];
  exportDiagnostics: ServiceClient["exportDiagnostics"];
  getSpeedLimit: ServiceClient["getSpeedLimit"];
  setSpeedLimit: ServiceClient["setSpeedLimit"];
  setDownloadSpeedLimit(jobId: JobId, bytesPerSecond: string | null): Promise<DownloadSnapshot>;
  /** Queue commands refresh `queues` after every attempt. */
  createQueue(name: string, maxActive?: number): Promise<QueueInfo>;
  renameQueue(queueId: string, name: string): Promise<QueueInfo>;
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo>;
  setQueueSchedule(queueId: string, schedule: QueueSchedule | null): Promise<QueueInfo>;
  deleteQueue(queueId: string, moveTo?: string): Promise<void>;
  startQueue(queueId: string): Promise<QueueInfo>;
  stopQueue(queueId: string): Promise<QueueInfo>;
  /** History retention (B2.12). Reads reject with a ServiceCallError; mutations never retry. */
  getHistory: ServiceClient["getHistory"];
  setHistory: ServiceClient["setHistory"];
  previewHistory: ServiceClient["previewHistory"];
  cleanHistory: ServiceClient["cleanHistory"];
  /** Retry and Refresh link (B2.16/B2.17): never repeated automatically; the caller repeats an
   * uncertain one with the same operation ID. */
  retry: ServiceClient["retryDownload"];
  replaceSource: ServiceClient["replaceSource"];
  /** Open containing folder (B2.19), only on a user action. */
  reveal: ServiceClient["revealDownload"];
  /** CORE-09 notification settings; reads and mutations reject with a ServiceCallError. */
  getNotifications: ServiceClient["getNotifications"];
  setNotifications: ServiceClient["setNotifications"];
}

const EMPTY: DownloadsState = {
  items: [],
  loaded: false,
  stale: false,
  ended: null,
  queues: [],
  mode: "none",
  reconnecting: false,
};

const ServiceContext = createContext<ServiceState>({ status: "checking" });
const ShellContext = createContext<DesktopShell>(new BrowserShell());
const DownloadsContext = createContext<DownloadsState>(EMPTY);
const noop = () => Promise.reject(new Error("no ServiceProvider"));
const ActionsContext = createContext<ServiceActions>({
  reconnect: () => {},
  beginExit: () => {},
  cancelExit: () => {},
  add: noop,
  pause: noop,
  resume: noop,
  cancel: noop,
  remove: noop,
  confirmName: noop,
  setJobQueue: noop,
  setPriority: noop,
  moveJob: noop,
  checkLink: noop,
  getSettings: noop,
  setSettings: noop,
  getConcurrency: noop,
  reloadQueues: noop,
  setConcurrency: noop,
  browserConnections: noop,
  repairBrowser: noop,
  listCredentials: noop,
  forgetCredential: noop,
  previewDiagnostics: noop,
  exportDiagnostics: noop,
  getSpeedLimit: noop,
  setSpeedLimit: noop,
  setDownloadSpeedLimit: noop,
  createQueue: noop,
  renameQueue: noop,
  setQueueLimit: noop,
  setQueueSchedule: noop,
  deleteQueue: noop,
  startQueue: noop,
  stopQueue: noop,
  getHistory: noop,
  setHistory: noop,
  previewHistory: noop,
  cleanHistory: noop,
  retry: noop,
  replaceSource: noop,
  reveal: noop,
  getNotifications: noop,
  setNotifications: noop,
});

export function ServiceProvider({
  children,
  client: injected,
  shell: injectedShell,
  pollIntervalMs = POLL_INTERVAL_MS,
  reconnectDelaysMs = RECONNECT_DELAYS_MS,
}: {
  children: ReactNode;
  /** Tests inject a fake; production uses the default client. */
  client?: ServiceClient;
  /** Tests inject a fake; production uses the Tauri shell or, in a plain browser, none. */
  shell?: DesktopShell;
  pollIntervalMs?: number;
  reconnectDelaysMs?: readonly number[];
}) {
  const [client] = useState(() => injected ?? createServiceClient());
  const [shell] = useState(() => injectedShell ?? createDesktopShell());
  const [state, setState] = useState<ServiceState>({ status: "checking" });
  const [downloads, setDownloads] = useState<DownloadsState>(EMPTY);
  // Bumped on every (re)connection: answers from an older connection are ignored.
  const [generation, setGeneration] = useState(0);
  // How the current connection is attached: decided by the subscription attempt.
  const [mode, setMode] = useState<LiveMode>("none");
  // The service went away while connected: reconnect automatically (attach-only).
  const [lost, setLost] = useState(false);
  // "Exit and stop downloads" began: nothing reconnects or resubscribes (B2.10).
  const exiting = useRef(false);
  // The next connection only attaches (never starts the service): set after an exit attempt.
  const attachOnly = useRef(false);
  // Ordering of list calls against mutations (polling): a list that started before the latest
  // acknowledged mutation may describe the job before it changed, so it is dropped.
  const seq = useRef(0);
  const lastMutation = useRef(0);
  // Polling: run the next list now (or right after the one in flight).
  const kick = useRef<() => void>(() => {});
  // Jobs this window saw removed: a late update can never bring them back.
  const removed = useRef(new Set<string>());
  // Applies queued events now (set by the live subscription). Called before a mutation's
  // acknowledgement is applied, so an older queued event never overwrites it.
  const flushEvents = useRef<() => void>(() => {});

  // Queue lists in flight: only the latest started one is applied, so an older answer that
  // arrives late never replaces a newer one (B2.31).
  const queuesSeq = useRef(0);
  const refreshQueues = useCallback(async () => {
    const mine = ++queuesSeq.current;
    try {
      const queues = await client.listQueues();
      if (mine === queuesSeq.current) setDownloads((d) => ({ ...d, queues }));
    } catch {
      // Queues stay as last known; the next snapshot tries again.
    }
  }, [client]);

  const goUnavailable = useCallback((failure: ServiceCallError) => {
    setDownloads((d) => ({ ...d, stale: true }));
    setMode("none");
    setState({ status: "unavailable", reason: "not_running", error: failure.error });
    if (!exiting.current) setLost(true);
  }, []);

  useEffect(() => {
    let active = true;
    // A user-requested (or first) connection may start the service; once exiting it only looks.
    const look = exiting.current || attachOnly.current;
    attachOnly.current = false;
    (look ? client.probe() : client.connect()).then((next) => {
      if (active) setState(next);
    });
    return () => {
      active = false;
    };
  }, [client, generation]);

  const connected = state.status === "connected";

  // B2.9: subscribe while connected. The start replaces the list; then each event must carry the
  // same service instance and the next sequence, otherwise a fresh subscription (and snapshot)
  // is taken. A service that refuses subscriptions is polled instead (explicit fallback).
  useEffect(() => {
    if (!connected) return;
    let active = true;
    let current: Subscription | null = null;
    let token = 0;
    let instance = "";
    let last = 0n;
    // Stream restarts in a row without an applied event, and the pending delayed restart.
    let restarts = 0;
    let restartTimer: ReturnType<typeof setTimeout> | undefined;
    const reopen = () => {
      if (restartTimer !== undefined) return;
      restarts += 1;
      const over = restarts - RESUBSCRIBE_FREE;
      if (over <= 0) {
        void open();
        return;
      }
      const delay = Math.min(RESUBSCRIBE_BACKOFF_MS * 2 ** (over - 1), RESUBSCRIBE_MAX_MS);
      restartTimer = setTimeout(() => {
        restartTimer = undefined;
        void open();
      }, delay);
    };
    // Pending changes per job (bounded by the number of jobs) and the flush timer.
    const pending = new Map<string, DownloadSnapshot | null>();
    let flushTimer: ReturnType<typeof setTimeout> | undefined;
    const flush = () => {
      clearTimeout(flushTimer);
      flushTimer = undefined;
      if (!active || pending.size === 0) return;
      const batch = new Map(pending);
      pending.clear();
      setDownloads((d) => applyBatch(d, batch));
    };
    const schedule = () => {
      if (flushTimer !== undefined) return;
      const hidden = typeof document !== "undefined" && document.visibilityState === "hidden";
      flushTimer = setTimeout(flush, hidden ? HIDDEN_FLUSH_MS : VISIBLE_FLUSH_MS);
    };
    flushEvents.current = flush;
    const onVisible = () => {
      if (document.visibilityState === "visible") flush();
    };
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", onVisible);

    const apply = (event: ServiceEvent) => {
      if (event.kind === "job_removed") {
        if (event.job_id) {
          removed.current.add(event.job_id);
          pending.set(event.job_id, null);
          schedule();
        }
        return;
      }
      const job = event.snapshot;
      if (!job || removed.current.has(job.job_id)) return;
      if (job.removed) removed.current.add(job.job_id);
      pending.set(job.job_id, job);
      schedule();
    };

    const open = async () => {
      current?.close();
      current = null;
      if (!active || exiting.current) return;
      const mine = ++token;
      try {
        const sub = await client.subscribe({
          onEvent: (event) => {
            if (!active || mine !== token) return;
            const next = BigInt(event.sequence);
            if (event.service_instance_id !== instance || next !== last + 1n) {
              reopen(); // gap or another service: a fresh authoritative snapshot
              return;
            }
            last = next;
            if (event.kind === "snapshot_required") {
              reopen();
              return;
            }
            restarts = 0;
            apply(event);
          },
          onEnded: () => {
            if (!active || mine !== token) return;
            reopen();
          },
        });
        if (!active || mine !== token) {
          sub.close();
          return;
        }
        current = sub;
        instance = sub.start.service_instance_id;
        last = BigInt(sub.start.sequence);
        // The snapshot is authoritative: changes queued from the previous stream are dropped.
        pending.clear();
        const items = sub.start.items.filter((x) => !removed.current.has(x.job_id));
        setDownloads((d) => withItems(d, items));
        setMode("stream");
        void refreshQueues();
      } catch (thrown) {
        if (!active || mine !== token) return;
        const failure = toCallError(thrown);
        if (failure.outcome === "rejected" && failure.error.kind === "INVALID_REQUEST") {
          // An older service without events: polling is the explicit fallback.
          setMode("polling");
          void refreshQueues();
        } else if (failure.outcome === "rejected") {
          // The service runs but its list cannot be read here (e.g. larger than the window
          // accepts). Polling would fail the same way, so nothing repeats automatically.
          setDownloads((d) => ({ ...d, stale: true }));
          setMode("none");
          setState({ status: "unavailable", reason: "list_unreadable", error: failure.error });
        } else {
          goUnavailable(failure);
        }
      }
    };
    void open();
    return () => {
      active = false;
      token += 1;
      clearTimeout(flushTimer);
      clearTimeout(restartTimer);
      pending.clear();
      flushEvents.current = () => {};
      if (typeof document !== "undefined")
        document.removeEventListener("visibilitychange", onVisible);
      current?.close();
      setMode("none");
    };
  }, [client, connected, generation, refreshQueues, goUnavailable]);

  // CORE-08: a queue's schedule state names its next change; refresh the queues just after the
  // earliest one passes (bounded to 1 s–60 s), so waiting reasons never show a past window.
  const nextScheduleChange = downloads.queues
    .flatMap((q) => [q.schedule_state?.next_change_at, q.schedule_state?.catch_up_until])
    .filter((x): x is string => typeof x === "string")
    .map((x) => Date.parse(x))
    .filter((x) => Number.isFinite(x))
    .reduce((a, b) => Math.min(a, b), Number.POSITIVE_INFINITY);
  useEffect(() => {
    if (!connected || !Number.isFinite(nextScheduleChange)) return;
    const wait = Math.min(Math.max(nextScheduleChange - Date.now() + 1000, 1000), 60_000);
    const timer = setTimeout(() => void refreshQueues(), wait);
    return () => clearTimeout(timer);
  }, [connected, nextScheduleChange, refreshQueues]);

  // Polling fallback: only for a service without events.
  useEffect(() => {
    if (!connected || mode !== "polling") return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let inFlight = false;
    let again = false;
    const tick = async () => {
      clearTimeout(timer);
      if (inFlight) {
        again = true;
        return;
      }
      inFlight = true;
      const started = ++seq.current;
      try {
        const items = await client.listDownloads();
        if (!active) return;
        if (started > lastMutation.current) {
          setDownloads((d) => withItems(d, items));
        } else {
          again = true;
        }
      } catch (thrown) {
        if (!active) return;
        const failure: ServiceCallError = toCallError(thrown);
        setDownloads((d) => ({ ...d, stale: true }));
        if (failure.outcome === "unavailable") {
          goUnavailable(failure);
          return;
        }
      } finally {
        inFlight = false;
      }
      if (!active) return;
      if (again) {
        again = false;
        void tick();
      } else {
        timer = setTimeout(() => void tick(), pollIntervalMs);
      }
    };
    kick.current = () => void tick();
    void tick();
    return () => {
      active = false;
      clearTimeout(timer);
      kick.current = () => {};
    };
  }, [client, connected, mode, generation, pollIntervalMs, goUnavailable]);

  // B2.10: bounded, attach-only reconnection after a loss. It never starts the service and
  // stops for good once "Exit and stop downloads" began.
  useEffect(() => {
    if (!lost || connected) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const attempt = (i: number) => {
      if (!active || exiting.current || i >= reconnectDelaysMs.length) {
        setLost(false);
        return;
      }
      timer = setTimeout(() => {
        void client.probe().then((next) => {
          if (!active || exiting.current) return;
          if (next.status === "connected") {
            setLost(false);
            setState(next);
            // Re-attach for a fresh subscription; still never start the service.
            attachOnly.current = true;
            setGeneration((g) => g + 1);
          } else {
            attempt(i + 1);
          }
        });
      }, reconnectDelaysMs[i]);
    };
    attempt(0);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [client, lost, connected, reconnectDelaysMs]);

  const mutate = useCallback(async (call: () => Promise<DownloadSnapshot>) => {
    try {
      const job = await call();
      flushEvents.current();
      lastMutation.current = ++seq.current;
      if (job.removed) removed.current.add(job.job_id);
      setDownloads((d) => ({ ...d, items: upsert(d.items, job) }));
      return job;
    } catch (thrown) {
      throw toCallError(thrown, true);
    } finally {
      kick.current();
    }
  }, []);

  const queueCommand = useCallback(
    async <T,>(call: () => Promise<T>): Promise<T> => {
      try {
        return await call();
      } catch (thrown) {
        throw toCallError(thrown, true);
      } finally {
        void refreshQueues();
        kick.current();
      }
    },
    [refreshQueues],
  );

  // Without a connection, whatever was listed last is shown as possibly out of date.
  const shown = useMemo<DownloadsState>(() => {
    const live = { ...downloads, mode, reconnecting: lost && !connected };
    return connected || !live.loaded ? live : { ...live, stale: true };
  }, [connected, downloads, mode, lost]);

  const actions = useMemo<ServiceActions>(
    () => ({
      reconnect: () => {
        exiting.current = false;
        setLost(false);
        setState({ status: "checking" });
        setGeneration((g) => g + 1);
      },
      beginExit: () => {
        exiting.current = true;
        setLost(false);
        setMode("none");
        setGeneration((g) => g + 1);
      },
      cancelExit: () => {
        exiting.current = false;
        attachOnly.current = true;
        setGeneration((g) => g + 1);
      },
      add: (url, key, options) => mutate(() => client.addDownload(url, key, options)),
      pause: (id) => mutate(() => client.pauseDownload(id)),
      resume: (id) => mutate(() => client.resumeDownload(id)),
      cancel: (id) => mutate(() => client.cancelDownload(id)),
      remove: (id) => mutate(() => client.removeDownload(id)),
      confirmName: (id) => mutate(() => client.confirmName(id)),
      setJobQueue: (id, queueId) => mutate(() => client.setJobQueue(id, queueId)),
      setPriority: (id, priority) => mutate(() => client.setPriority(id, priority)),
      moveJob: (id, to) => mutate(() => client.moveJob(id, to)),
      checkLink: async (url) => {
        try {
          return await client.checkLink(url);
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      getSettings: async () => {
        try {
          return await client.getSettings();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      setSettings: (settings) => queueCommand(() => client.setSettings(settings)),
      reloadQueues: refreshQueues,
      getConcurrency: async () => {
        try {
          return await client.getConcurrency();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      setConcurrency: (max) => queueCommand(() => client.setConcurrency(max)),
      browserConnections: async () => {
        try {
          return await client.browserConnections();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      repairBrowser: async (browser) => {
        try {
          return await client.repairBrowser(browser);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      listCredentials: async () => {
        try {
          return await client.listCredentials();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      forgetCredential: async (id) => {
        try {
          return await client.forgetCredential(id);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      previewDiagnostics: async (includeCrash) => {
        try {
          return await client.previewDiagnostics(includeCrash);
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      exportDiagnostics: async (id, title) => {
        try {
          return await client.exportDiagnostics(id, title);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      getSpeedLimit: async () => {
        try {
          return await client.getSpeedLimit();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      setSpeedLimit: async (bps) => {
        try {
          return await client.setSpeedLimit(bps);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      setDownloadSpeedLimit: (id, bps) => mutate(() => client.setDownloadSpeedLimit(id, bps)),
      createQueue: (name, max) => queueCommand(() => client.createQueue(name, max)),
      renameQueue: (id, name) => queueCommand(() => client.renameQueue(id, name)),
      setQueueLimit: (id, max) => queueCommand(() => client.setQueueLimit(id, max)),
      setQueueSchedule: (id, s) => queueCommand(() => client.setQueueSchedule(id, s)),
      deleteQueue: (id, moveTo) => queueCommand(() => client.deleteQueue(id, moveTo)),
      startQueue: (id) => queueCommand(() => client.startQueue(id)),
      stopQueue: (id) => queueCommand(() => client.stopQueue(id)),
      getHistory: async () => {
        try {
          return await client.getHistory();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      setHistory: async (days) => {
        try {
          return await client.setHistory(days);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      previewHistory: async (days) => {
        try {
          return await client.previewHistory(days);
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      retry: (id, op, options) => mutate(() => client.retryDownload(id, op, options)),
      replaceSource: async (id, op, url, mode, expected) => {
        try {
          const result = await client.replaceSource(id, op, url, mode, expected);
          flushEvents.current();
          lastMutation.current = ++seq.current;
          setDownloads((d) => ({ ...d, items: upsert(d.items, result.job) }));
          return result;
        } catch (thrown) {
          throw toCallError(thrown, true);
        } finally {
          kick.current();
        }
      },
      reveal: async (id) => {
        try {
          return await client.revealDownload(id);
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      getNotifications: async () => {
        try {
          return await client.getNotifications();
        } catch (thrown) {
          throw toCallError(thrown);
        }
      },
      setNotifications: async (enabled, locale) => {
        try {
          return await client.setNotifications(enabled, locale);
        } catch (thrown) {
          throw toCallError(thrown, true);
        }
      },
      // Removals arrive as job_removed events (or the next poll in the fallback).
      cleanHistory: async () => {
        try {
          return await client.cleanHistory();
        } catch (thrown) {
          throw toCallError(thrown, true);
        } finally {
          kick.current();
        }
      },
    }),
    [client, mutate, queueCommand, refreshQueues],
  );

  return (
    <ServiceContext.Provider value={state}>
      <DownloadsContext.Provider value={shown}>
        <ActionsContext.Provider value={actions}>
          <ShellContext.Provider value={shell}>{children}</ShellContext.Provider>
        </ActionsContext.Provider>
      </DownloadsContext.Provider>
    </ServiceContext.Provider>
  );
}

export function useServiceState(): ServiceState {
  return useContext(ServiceContext);
}

export function useDownloads(): DownloadsState {
  return useContext(DownloadsContext);
}

export function useServiceActions(): ServiceActions {
  return useContext(ActionsContext);
}

export function useShell(): DesktopShell {
  return useContext(ShellContext);
}
