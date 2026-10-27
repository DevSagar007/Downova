// Test double for the service-client boundary. It records every call and lets a test decide how
// each one ends, including answers that arrive late (stale polling) or never (uncertain adds).
import type { DownloadSnapshot, ErrorData, HelloResult, JobId } from "@dm/contracts";
import {
  ServiceCallError,
  type CallOutcome,
  type ConnectionState,
  type ServiceClient,
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
  method: "connect" | "list" | "add" | "pause" | "resume" | "cancel";
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

  addDownload(url: string, key: string): Promise<DownloadSnapshot> {
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
