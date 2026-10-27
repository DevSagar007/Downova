// Service-client boundary: the ONLY module family in the UI allowed to reach the desktop shell
// (Tauri) and, through it, the background service. Features depend on the `ServiceClient`
// interface, never on transport details. Contract types come from @dm/contracts.
import type { DownloadSnapshot, ErrorData, HelloResult, JobId } from "@dm/contracts";
import { invoke, isTauri } from "@tauri-apps/api/core";

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

export interface ServiceClient {
  /** Finds (or, in the desktop app, starts) the service. Never throws for expected unavailability. */
  connect(): Promise<ConnectionState>;
  /** downloads.v1.list. Rejects with a ServiceCallError. */
  listDownloads(): Promise<DownloadSnapshot[]>;
  /** downloads.v1.add for one http(s) link with the caller's idempotency key. */
  addDownload(url: string, idempotencyKey: string): Promise<DownloadSnapshot>;
  pauseDownload(jobId: JobId): Promise<DownloadSnapshot>;
  resumeDownload(jobId: JobId): Promise<DownloadSnapshot>;
  cancelDownload(jobId: JobId): Promise<DownloadSnapshot>;
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

/**
 * Plain-browser client (`pnpm dev`): there is no desktop shell and therefore no service bridge.
 * It reports `not_implemented` and refuses every call instead of pretending to connect.
 */
export class FoundationServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    return { status: "unavailable", reason: "not_implemented", error: NOT_IMPLEMENTED };
  }
  private refuse(): Promise<never> {
    return Promise.reject(new ServiceCallError("unavailable", NOT_IMPLEMENTED));
  }
  listDownloads(): Promise<DownloadSnapshot[]> {
    return this.refuse();
  }
  addDownload(): Promise<DownloadSnapshot> {
    return this.refuse();
  }
  pauseDownload(): Promise<DownloadSnapshot> {
    return this.refuse();
  }
  resumeDownload(): Promise<DownloadSnapshot> {
    return this.refuse();
  }
  cancelDownload(): Promise<DownloadSnapshot> {
    return this.refuse();
  }
}

/**
 * Desktop client: the shell's `service_connect` finds or starts the per-user service; the five
 * `downloads_*` commands forward the fixed download operations (validated in Rust, SEC-05).
 */
export class TauriServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    try {
      return await invoke<ConnectionState>("service_connect");
    } catch {
      return {
        status: "unavailable",
        reason: "not_running",
        error: {
          kind: "SERVICE_UNAVAILABLE",
          retryable: true,
          next_action: "retry",
          detail: null,
        },
      };
    }
  }

  async listDownloads(): Promise<DownloadSnapshot[]> {
    try {
      return (await invoke<{ items: DownloadSnapshot[] }>("downloads_list")).items;
    } catch (e) {
      throw toCallError(e);
    }
  }

  addDownload(url: string, idempotencyKey: string): Promise<DownloadSnapshot> {
    return mutate("downloads_add", { url, idempotencyKey });
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
}

async function mutate(command: string, args: Record<string, string>): Promise<DownloadSnapshot> {
  try {
    return await invoke<DownloadSnapshot>(command, args);
  } catch (e) {
    throw toCallError(e, true);
  }
}

export function createServiceClient(): ServiceClient {
  // In a plain browser (`pnpm dev`) there is no shell and no service bridge.
  return isTauri() ? new TauriServiceClient() : new FoundationServiceClient();
}
