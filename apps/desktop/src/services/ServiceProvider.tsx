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
  /** Queue commands refresh `queues` after every attempt. */
  createQueue(name: string, maxActive?: number): Promise<QueueInfo>;
  renameQueue(queueId: string, name: string): Promise<QueueInfo>;
  setQueueLimit(queueId: string, maxActive: number): Promise<QueueInfo>;
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
  createQueue: noop,
  renameQueue: noop,
  setQueueLimit: noop,
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

  const refreshQueues = useCallback(async () => {
    try {
      const queues = await client.listQueues();
      setDownloads((d) => ({ ...d, queues }));
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

    const apply = (event: ServiceEvent) => {
      if (event.kind === "job_removed") {
        if (event.job_id) removed.current.add(event.job_id);
        setDownloads((d) => ({ ...d, items: d.items.filter((x) => x.job_id !== event.job_id) }));
        return;
      }
      const job = event.snapshot;
      if (!job || removed.current.has(job.job_id)) return;
      if (job.removed) removed.current.add(job.job_id);
      setDownloads((d) => withItems(d, upsert(d.items, job)));
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
              void open(); // gap or another service: a fresh authoritative snapshot
              return;
            }
            last = next;
            if (event.kind === "snapshot_required") {
              void open();
              return;
            }
            apply(event);
          },
          onEnded: () => {
            if (!active || mine !== token) return;
            void open();
          },
        });
        if (!active || mine !== token) {
          sub.close();
          return;
        }
        current = sub;
        instance = sub.start.service_instance_id;
        last = BigInt(sub.start.sequence);
        const items = sub.start.items.filter((x) => !removed.current.has(x.job_id));
        setDownloads((d) => withItems(d, items));
        setMode("stream");
        void refreshQueues();
      } catch (thrown) {
        if (!active || mine !== token) return;
        const failure = toCallError(thrown);
        if (failure.outcome === "rejected") {
          setMode("polling");
          void refreshQueues();
        } else {
          goUnavailable(failure);
        }
      }
    };
    void open();
    return () => {
      active = false;
      token += 1;
      current?.close();
      setMode("none");
    };
  }, [client, connected, generation, refreshQueues, goUnavailable]);

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
      createQueue: (name, max) => queueCommand(() => client.createQueue(name, max)),
      renameQueue: (id, name) => queueCommand(() => client.renameQueue(id, name)),
      setQueueLimit: (id, max) => queueCommand(() => client.setQueueLimit(id, max)),
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
    [client, mutate, queueCommand],
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
