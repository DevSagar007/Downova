import { describe, expect, it } from "vitest";
import type { DownloadSnapshot } from "@dm/contracts";
import { clearFinished, isFinished, type ClearTarget } from "./clear";
import { callError, deferred, snapshot } from "../../test/fake-service";

const targets = (...ids: string[]): ClearTarget[] =>
  ids.map((jobId) => ({ jobId, name: `${jobId}.bin` }));

describe("clear finished", () => {
  it("offers only completed, failed and cancelled jobs", () => {
    for (const phase of ["completed", "failed", "cancelled"] as const) {
      expect(isFinished(snapshot({ phase }))).toBe(true);
    }
    for (const phase of [
      "queued",
      "transferring",
      "verifying",
      "scanning",
      "finalizing",
    ] as const) {
      expect(isFinished(snapshot({ phase }))).toBe(false);
    }
    expect(isFinished(snapshot({ phase: "completed", removed: true }))).toBe(false);
  });

  it("runs removals with bounded concurrency", async () => {
    let active = 0;
    let peak = 0;
    const results: string[] = [];
    const remove = async (jobId: string): Promise<DownloadSnapshot> => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 5));
      active -= 1;
      results.push(jobId);
      return snapshot({ job_id: jobId, phase: "completed", removed: true });
    };
    const outcome = await clearFinished(targets("a", "b", "c", "d", "e", "f", "g"), remove);
    expect(peak).toBeLessThanOrEqual(3);
    expect(peak).toBeGreaterThan(1);
    expect(outcome.succeeded.sort()).toEqual(["a", "b", "c", "d", "e", "f", "g"]);
    expect(outcome.failed).toEqual([]);
    expect(results.sort()).toEqual(["a", "b", "c", "d", "e", "f", "g"]);
  });

  it("keeps failing rows and their actionable errors while other removals continue", async () => {
    const outcome = await clearFinished(targets("ok1", "bad", "ok2"), async (jobId) => {
      if (jobId === "bad") throw callError("rejected", "INVALID_REQUEST", "none");
      return snapshot({ job_id: jobId, phase: "completed", removed: true });
    });
    expect(outcome.succeeded).toEqual(["ok1", "ok2"]);
    expect(outcome.failed).toHaveLength(1);
    expect(outcome.failed[0]).toMatchObject({
      jobId: "bad",
      name: "bad.bin",
      error: { outcome: "rejected" },
    });
    expect(outcome.failed[0]?.error.error.kind).toBe("INVALID_REQUEST");
  });

  it("does nothing when the frozen scope is empty", async () => {
    let calls = 0;
    const outcome = await clearFinished([], async () => {
      calls += 1;
      return snapshot();
    });
    expect(outcome).toEqual({ succeeded: [], failed: [] });
    expect(calls).toBe(0);
  });

  it("reports an uncertain outcome honestly", async () => {
    const late = deferred<DownloadSnapshot>();
    const pending = clearFinished(targets("maybe"), () => late.promise);
    await Promise.resolve();
    late.reject(callError("uncertain"));
    const outcome = await pending;
    expect(outcome.succeeded).toEqual([]);
    expect(outcome.failed[0]?.error.outcome).toBe("uncertain");
  });
});
