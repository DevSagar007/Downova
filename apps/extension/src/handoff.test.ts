import { describe as suite, expect, it } from "vitest";
import type { NativeHostAdd } from "@dm/contracts";
import {
  Handoffs,
  STALE_SENDING_MS,
  checkLink,
  interpret,
  settleStale,
  type Deps,
  type HandoffRecord,
  type SendResult,
} from "./handoff.js";

function harness(replies: ((m: NativeHostAdd) => SendResult)[]) {
  let stored: HandoffRecord[] = [];
  let n = 0;
  const sent: NativeHostAdd[] = [];
  const deps: Deps = {
    load: async () => structuredClone(stored),
    save: async (r) => {
      stored = structuredClone(r);
    },
    send: async (m) => {
      sent.push(m);
      const reply = replies.shift();
      if (!reply) throw new Error("unexpected send");
      return reply(m);
    },
    now: () => 1_000,
    token: () => `tok${String(++n).padStart(13, "0")}`,
  };
  return { handoffs: new Handoffs(deps), sent, stored: () => stored };
}

const accepted = (m: NativeHostAdd): SendResult => ({
  kind: "response",
  value: { v: 1, request_id: m.request_id, outcome: "accepted", job: { job_id: "job_1" } },
});

suite("checkLink", () => {
  it("accepts public http(s) links", () => {
    for (const ok of ["https://example.org/f.zip", "http://h:8080/a?b=c#d", "https://[::1]/x"])
      expect(checkLink(ok).ok, ok).toBe(true);
  });
  it("rejects other schemes, credentials and malformed links", () => {
    for (const bad of [
      "ftp://h/f",
      "file:///etc/passwd",
      "javascript:alert(1)",
      "data:text/plain,x",
      "blob:https://h/1",
      "chrome://downloads",
      "https://user:pw@h/f",
      "https://user@h/f",
      "https://h/f g",
      "not a url",
      `https://h/${"a".repeat(8192)}`,
    ])
      expect(checkLink(bad).ok, bad).toBe(false);
  });
});

suite("interpret", () => {
  it("keeps service acceptance distinct from a started transfer", () => {
    const r = interpret(accepted({ request_id: "req_1" } as NativeHostAdd), "req_1");
    expect(r.state).toBe("accepted");
    expect(r.jobId).toBe("job_1");
    expect(r.detail).toMatch(/not necessarily started/);
  });
  it("maps a missing host to unavailable and other failures to uncertain", () => {
    const missing = {
      kind: "error",
      message: "Specified native messaging host not found.",
    } as const;
    expect(interpret(missing, "r").state).toBe("unavailable");
    expect(interpret({ kind: "error", message: "Native host has exited." }, "r").state).toBe(
      "uncertain",
    );
    expect(interpret({ kind: "timeout" }, "r").state).toBe("uncertain");
  });
  it("treats malformed or mismatched replies as uncertain, not failed", () => {
    for (const value of [
      null,
      "x",
      { v: 2, request_id: "r", outcome: "accepted" },
      { v: 1, request_id: "other", outcome: "accepted", job: { job_id: "j" } },
      { v: 1, request_id: "r", outcome: "accepted" },
      { v: 1, request_id: "r", outcome: "weird" },
    ])
      expect(interpret({ kind: "response", value }, "r").state).toBe("uncertain");
  });
  it("passes the host's actionable detail through", () => {
    const value = {
      v: 1,
      request_id: "r",
      outcome: "unavailable",
      error: { kind: "SERVICE_UNAVAILABLE", detail: "open the Downova app, then try again" },
    };
    expect(interpret({ kind: "response", value }, "r")).toMatchObject({
      state: "unavailable",
      detail: "Open the Downova app, then try again.",
    });
  });
});

suite("Handoffs", () => {
  it("sends one add with a fresh idempotency key and records acceptance", async () => {
    const h = harness([accepted]);
    const r = await h.handoffs.start("https://example.org/f.zip");
    expect(r.state).toBe("accepted");
    expect(h.sent).toHaveLength(1);
    expect(h.sent[0]).toMatchObject({ v: 1, op: "add", url: "https://example.org/f.zip" });
    expect(h.sent[0]?.idempotency_key).toBe(r.idempotencyKey);
    // Nothing beyond the contract: no cookies, headers or referrer.
    expect(Object.keys(h.sent[0] ?? {}).sort()).toEqual([
      "idempotency_key",
      "op",
      "request_id",
      "url",
      "v",
    ]);
  });

  it("rejects unsupported links locally without contacting the host", async () => {
    const h = harness([]);
    const r = await h.handoffs.start("ftp://h/f");
    expect(r.state).toBe("rejected");
    expect(h.sent).toHaveLength(0);
    expect(await h.handoffs.retry(r.id)).toBeNull();
  });

  it("a timeout is uncertain and the retry reuses the key with a new request id", async () => {
    const h = harness([() => ({ kind: "timeout" }), accepted]);
    const first = await h.handoffs.start("https://example.org/f.zip");
    expect(first.state).toBe("uncertain");
    const second = await h.handoffs.retry(first.id);
    expect(second?.state).toBe("accepted");
    expect(second?.attempts).toBe(2);
    expect(h.sent[1]?.idempotency_key).toBe(h.sent[0]?.idempotency_key);
    expect(h.sent[1]?.request_id).not.toBe(h.sent[0]?.request_id);
    // A settled hand-off is not sent again.
    expect(await h.handoffs.retry(first.id)).toBeNull();
  });

  it("persists the key before sending, so a stopped worker leaves a retryable record", async () => {
    let duringSend: HandoffRecord[] = [];
    const h = harness([
      () => {
        duringSend = h.stored();
        return { kind: "timeout" };
      },
    ]);
    await h.handoffs.start("https://example.org/f.zip");
    const interrupted = duringSend[0] as HandoffRecord;
    expect(interrupted.state).toBe("sending");
    expect(interrupted.idempotencyKey).toMatch(/^ext_/);
    // A worker that died mid-request: "sending" until stale, then uncertain (retryable).
    expect(settleStale([interrupted], interrupted.updatedAt + 1)[0]?.state).toBe("sending");
    const settled = settleStale([interrupted], interrupted.updatedAt + STALE_SENDING_MS + 1)[0];
    expect(settled?.state).toBe("uncertain");
    expect(settled?.idempotencyKey).toBe(interrupted.idempotencyKey);
  });

  it("unavailable is retryable with the same key", async () => {
    const h = harness([
      () => ({ kind: "error", message: "Specified native messaging host not found." }),
      accepted,
    ]);
    const r = await h.handoffs.start("https://example.org/f.zip");
    expect(r.state).toBe("unavailable");
    await h.handoffs.retry(r.id);
    expect(h.sent[0]?.idempotency_key).toBe(h.sent[1]?.idempotency_key);
  });

  it("keeps a bounded history and dismisses settled records", async () => {
    const h = harness(Array.from({ length: 25 }, () => accepted));
    for (let i = 0; i < 25; i++) await h.handoffs.start(`https://example.org/${i}`);
    expect(h.stored()).toHaveLength(20);
    const first = h.stored()[0] as HandoffRecord;
    await h.handoffs.dismiss(first.id);
    expect(h.stored().some((r) => r.id === first.id)).toBe(false);
  });
});
