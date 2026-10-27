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
import type { DownloadSnapshot, JobId } from "@dm/contracts";
import {
  BrowserShell,
  createDesktopShell,
  createServiceClient,
  type DesktopShell,
  toCallError,
  type ConnectionState,
  type ServiceCallError,
  type ServiceClient,
} from "./service-client";

type ServiceState = { status: "checking" } | ConnectionState;

/** How often the list is refreshed while connected (B2.1 polls; ARCH-08 events come later). */
export const POLL_INTERVAL_MS = 1000;

export interface DownloadsState {
  /** The service's snapshots, in service order. Kept (and marked stale) when disconnected. */
  items: DownloadSnapshot[];
  /** At least one list has arrived since the app started. */
  loaded: boolean;
  /** The items may be out of date: the last list attempt failed or the service is unreachable. */
  stale: boolean;
}

export interface ServiceActions {
  /** Connects again (the desktop shell finds or starts the service). */
  reconnect(): void;
  /** Each resolves with the service's acknowledgement or rejects with a ServiceCallError. They
   * are never repeated automatically; a refresh follows every attempt. */
  add(url: string, idempotencyKey: string): Promise<DownloadSnapshot>;
  pause(jobId: JobId): Promise<DownloadSnapshot>;
  resume(jobId: JobId): Promise<DownloadSnapshot>;
  cancel(jobId: JobId): Promise<DownloadSnapshot>;
}

const ServiceContext = createContext<ServiceState>({ status: "checking" });
const ShellContext = createContext<DesktopShell>(new BrowserShell());
const DownloadsContext = createContext<DownloadsState>({ items: [], loaded: false, stale: false });
const noop = () => Promise.reject(new Error("no ServiceProvider"));
const ActionsContext = createContext<ServiceActions>({
  reconnect: () => {},
  add: noop,
  pause: noop,
  resume: noop,
  cancel: noop,
});

export function ServiceProvider({
  children,
  client: injected,
  shell: injectedShell,
  pollIntervalMs = POLL_INTERVAL_MS,
}: {
  children: ReactNode;
  /** Tests inject a fake; production uses the default client. */
  client?: ServiceClient;
  /** Tests inject a fake; production uses the Tauri shell or, in a plain browser, none. */
  shell?: DesktopShell;
  pollIntervalMs?: number;
}) {
  const [client] = useState(() => injected ?? createServiceClient());
  const [shell] = useState(() => injectedShell ?? createDesktopShell());
  const [state, setState] = useState<ServiceState>({ status: "checking" });
  const [downloads, setDownloads] = useState<DownloadsState>({
    items: [],
    loaded: false,
    stale: false,
  });
  // Bumped on every (re)connection: answers from an older connection are ignored.
  const [generation, setGeneration] = useState(0);
  // Ordering of list calls against mutations: a list that started before the latest
  // acknowledged mutation may describe the job before it changed, so it is dropped.
  const seq = useRef(0);
  const lastMutation = useRef(0);
  // Set by the polling loop: run the next list now (or right after the one in flight).
  const kick = useRef<() => void>(() => {});

  useEffect(() => {
    let active = true;
    client.connect().then((next) => {
      if (active) setState(next);
    });
    return () => {
      active = false;
    };
  }, [client, generation]);

  const connected = state.status === "connected";
  useEffect(() => {
    if (!connected) return;
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
          setDownloads({ items, loaded: true, stale: false });
        } else {
          again = true;
        }
      } catch (thrown) {
        if (!active) return;
        const failure: ServiceCallError = toCallError(thrown);
        setDownloads((d) => ({ ...d, stale: true }));
        if (failure.outcome === "unavailable") {
          // Stop polling; the user (or a later reconnect) brings the service back.
          setState({ status: "unavailable", reason: "not_running", error: failure.error });
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
  }, [client, connected, generation, pollIntervalMs]);

  const mutate = useCallback(async (call: () => Promise<DownloadSnapshot>) => {
    try {
      const job = await call();
      lastMutation.current = ++seq.current;
      setDownloads((d) => ({
        ...d,
        items: d.items.some((x) => x.job_id === job.job_id)
          ? d.items.map((x) => (x.job_id === job.job_id ? job : x))
          : [...d.items, job],
      }));
      return job;
    } catch (thrown) {
      throw toCallError(thrown, true);
    } finally {
      kick.current();
    }
  }, []);

  // Without a connection, whatever was listed last is shown as possibly out of date.
  const shown = useMemo<DownloadsState>(
    () => (connected || !downloads.loaded ? downloads : { ...downloads, stale: true }),
    [connected, downloads],
  );

  const actions = useMemo<ServiceActions>(
    () => ({
      reconnect: () => {
        setState({ status: "checking" });
        setGeneration((g) => g + 1);
      },
      add: (url, key) => mutate(() => client.addDownload(url, key)),
      pause: (id) => mutate(() => client.pauseDownload(id)),
      resume: (id) => mutate(() => client.resumeDownload(id)),
      cancel: (id) => mutate(() => client.cancelDownload(id)),
    }),
    [client, mutate],
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
