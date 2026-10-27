// The Tauri client at the bridge boundary: command names, argument shapes and the mapping of the
// shell's BridgeError into ServiceCallError (including cancel, which has no UI control yet).
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ServiceClient } from "./service-client";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));

const { TauriServiceClient, FoundationServiceClient, ServiceCallError, toCallError } =
  await import("./service-client");

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
