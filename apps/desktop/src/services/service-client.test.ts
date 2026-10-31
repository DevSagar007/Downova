// The Tauri client at the bridge boundary: command names, argument shapes and the mapping of the
// shell's BridgeError into ServiceCallError (including cancel, which has no UI control yet).
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ServiceClient } from "./service-client";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
// Window events from the shell: tests deliver them through `emit`.
const listeners = new Map<string, Set<(e: { payload: unknown }) => void>>();
const emit = (name: string, payload: unknown) => {
  for (const l of listeners.get(name) ?? []) l({ payload });
};
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: (e: { payload: unknown }) => void) => {
    const set = listeners.get(name) ?? new Set();
    set.add(handler);
    listeners.set(name, set);
    return () => set.delete(handler);
  },
}));

const {
  TauriServiceClient,
  FoundationServiceClient,
  ServiceCallError,
  toCallError,
  SERVICE_EVENT,
  STREAM_ENDED,
} = await import("./service-client");

const job = { job_id: "job_1" };

beforeEach(() => {
  invoke.mockReset();
});

describe("TauriServiceClient", () => {
  it("calls only the fixed download commands with typed arguments", async () => {
    const client = new TauriServiceClient();
    invoke.mockResolvedValueOnce({ items: [job] });
    expect(await client.listDownloads()).toEqual([job]);
    invoke.mockResolvedValue(job);
    await client.addDownload("https://h/f", "ui-key-0001");
    await client.pauseDownload("job_1");
    await client.resumeDownload("job_1");
    await client.cancelDownload("job_1");
    expect(invoke.mock.calls).toEqual([
      ["downloads_list"],
      ["downloads_add", { url: "https://h/f", idempotencyKey: "ui-key-0001" }],
      ["downloads_pause", { jobId: "job_1" }],
      ["downloads_resume", { jobId: "job_1" }],
      ["downloads_cancel", { jobId: "job_1" }],
    ]);
  });

  it("keeps the shell's outcome and structured error", async () => {
    const client = new TauriServiceClient();
    const error = {
      kind: "INVALID_REQUEST",
      retryable: false,
      next_action: "none",
      detail: "job is completed",
    };
    invoke.mockRejectedValueOnce({ outcome: "rejected", error });
    const refused = await client.cancelDownload("job_1").catch((e: unknown) => e);
    expect(refused).toBeInstanceOf(ServiceCallError);
    expect(refused).toMatchObject({ outcome: "rejected", error });
    invoke.mockRejectedValueOnce({
      outcome: "uncertain",
      error: { ...error, kind: "SERVICE_UNAVAILABLE" },
    });
    await expect(client.addDownload("https://h/f", "ui-key-0001")).rejects.toMatchObject({
      outcome: "uncertain",
    });
  });

  it("treats an unexpected bridge failure as unavailable for reads and uncertain for mutations", async () => {
    const client = new TauriServiceClient();
    invoke.mockImplementation(() => Promise.reject(new Error("command downloads_add not allowed")));
    const settle = (p: Promise<unknown>) =>
      p.then(
        () => null,
        (e: unknown) => e,
      );
    expect(await settle(client.listDownloads())).toMatchObject({ outcome: "unavailable" });
    expect(await settle(client.cancelDownload("job_1"))).toMatchObject({ outcome: "uncertain" });
    expect(toCallError({ outcome: "bogus", error: {} }).outcome).toBe("unavailable");
  });
});

describe("TauriServiceClient (B2.8–B2.11)", () => {
  it("sends only the add options that were chosen, so old behaviour stays the default", async () => {
    const client = new TauriServiceClient();
    invoke.mockResolvedValue(job);
    await client.addDownload("https://h/f", "k1");
    await client.addDownload("https://h/f", "k2", { onDuplicate: "refuse", queueId: "q_1" });
    expect(invoke.mock.calls).toEqual([
      ["downloads_add", { url: "https://h/f", idempotencyKey: "k1" }],
      [
        "downloads_add",
        { url: "https://h/f", idempotencyKey: "k2", onDuplicate: "refuse", queueId: "q_1" },
      ],
    ]);
  });

  it("delivers only its own subscription's events, including those sent before it knew its number", async () => {
    const client = new TauriServiceClient();
    const events: string[] = [];
    const ended: string[] = [];
    const ev = (subscription: number, sequence: string) => ({
      subscription,
      event: {
        service_instance_id: "s",
        sequence,
        kind: "progress",
        job_id: null,
        attempt_id: null,
        snapshot: null,
      },
    });
    invoke.mockImplementationOnce(async () => {
      // The shell's task may forward events before the command returns.
      emit(SERVICE_EVENT, ev(7, "11"));
      emit(SERVICE_EVENT, ev(6, "99")); // an older subscription
      return { subscription: 7, start: { service_instance_id: "s", sequence: "10", items: [] } };
    });
    const sub = await client.subscribe({
      onEvent: (e) => events.push(e.sequence),
      onEnded: (r) => ended.push(r),
    });
    expect(sub.start.sequence).toBe("10");
    await Promise.resolve();
    emit(SERVICE_EVENT, ev(7, "12"));
    emit(STREAM_ENDED, { subscription: 6, reason: "lost" });
    emit(STREAM_ENDED, { subscription: 7, reason: "snapshot_required" });
    emit(SERVICE_EVENT, ev(7, "13")); // after the end: ignored
    expect(events).toEqual(["11", "12"]);
    expect(ended).toEqual(["snapshot_required"]);
    invoke.mockResolvedValue(undefined);
    sub.close();
    expect(invoke).toHaveBeenLastCalledWith("events_unsubscribe");
  });

  it("reports a service without events as rejected, so the caller may poll", async () => {
    const client = new TauriServiceClient();
    invoke.mockRejectedValueOnce({
      outcome: "rejected",
      error: { kind: "INVALID_REQUEST", retryable: false, next_action: "none", detail: null },
    });
    await expect(client.subscribe({ onEvent: () => {}, onEnded: () => {} })).rejects.toMatchObject({
      outcome: "rejected",
    });
    expect(listeners.get(SERVICE_EVENT)?.size ?? 0).toBe(0);
  });

  it("probes without starting the service", async () => {
    const client = new TauriServiceClient();
    invoke.mockRejectedValueOnce(new Error("no bridge"));
    expect(await client.probe()).toMatchObject({ status: "unavailable", reason: "not_running" });
    expect(invoke).toHaveBeenCalledWith("service_probe");
  });
});

describe("FoundationServiceClient (plain browser)", () => {
  it("never pretends to reach a service", async () => {
    const client: ServiceClient = new FoundationServiceClient();
    expect(await client.connect()).toMatchObject({
      status: "unavailable",
      reason: "not_implemented",
    });
    await expect(client.listDownloads()).rejects.toMatchObject({ outcome: "unavailable" });
    await expect(client.cancelDownload("job_1")).rejects.toMatchObject({ outcome: "unavailable" });
    expect(invoke).not.toHaveBeenCalled();
  });
});
