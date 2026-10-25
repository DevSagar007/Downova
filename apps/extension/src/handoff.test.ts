import { describe as suite, expect, it } from "vitest";
import type { NativeHostAdd, NativeHostRequest } from "@dm/contracts";
import {
  Handoffs,
  RETENTION_MS,
  STALE_SENDING_MS,
  checkLink,
  displayUrl,
  interpret,
  interpretHello,
  settleStale,
  type Deps,
  type HandoffRecord,
  type SendResult,
} from "./handoff.js";

function harness(replies: ((m: NativeHostAdd) => SendResult)[]) {
  let stored: HandoffRecord[] = [];
  let n = 0;
  let clock = 1_000;
  const sent: NativeHostAdd[] = [];
  const deps: Deps = {
    load: async () => structuredClone(stored),
    save: async (r) => {
      stored = structuredClone(r);
    },
    send: async (request: NativeHostRequest) => {
      const m = request as NativeHostAdd;
      sent.push(m);
      const reply = replies.shift();
      if (!reply) throw new Error("unexpected send");
      return reply(m);
    },
    now: () => clock,
    token: () => `tok${String(++n).padStart(13, "0")}`,
  };
  return {
    handoffs: new Handoffs(deps),
    sent,
    stored: () => stored,
    advance: (ms: number) => (clock += ms),
    /** A new worker instance over the same storage (the old one's memory is gone). */
    restart: () => new Handoffs(deps),
  };
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

suite("worker restarts, cleanup and display", () => {
  it("a restarted worker retries the stored hand-off with the same key and payload", async () => {
    const h = harness([() => ({ kind: "timeout" }), accepted]);
    const first = await h.handoffs.start("https://example.org/f.zip?sig=secret");
    expect(first.state).toBe("uncertain");
    const restarted = h.restart();
    const again = await restarted.retry(first.id);
    expect(again?.state).toBe("accepted");
    const [a, b] = h.sent;
    expect(b?.idempotency_key).toBe(a?.idempotency_key);
    expect(b?.url).toBe(a?.url);
    expect(b?.request_id).not.toBe(a?.request_id);
  });

  it("an interrupted send becomes retryable after a restart, a new action gets a new key", async () => {
    let interrupted: HandoffRecord[] = [];
    const h = harness([
      () => {
        interrupted = h.stored();
        return { kind: "timeout" };
      },
      accepted,
      accepted,
    ]);
    await h.handoffs.start("https://example.org/f.zip");
    // The worker died while sending: storage still says "sending".
    const stuck = interrupted[0] as HandoffRecord;
    const w2 = h.restart();
    h.advance(STALE_SENDING_MS + 1);
    const listed = await w2.list();
    expect(listed[0]?.state).toBe("uncertain");
    await w2.retry(stuck.id);
    expect(h.sent[1]?.idempotency_key).toBe(stuck.idempotencyKey);
    await w2.start("https://example.org/f.zip");
    expect(h.sent[2]?.idempotency_key).not.toBe(stuck.idempotencyKey);
  });

  it("the service being down is reported as unavailable with the host's advice", async () => {
    const h = harness([
      (m) => ({
        kind: "response",
        value: {
          v: 1,
          request_id: m.request_id,
          outcome: "unavailable",
          error: {
            kind: "SERVICE_UNAVAILABLE",
            retryable: true,
            next_action: "retry",
            detail:
              "the Downova download service is not running; open the Downova app, then try again",
          },
        },
      }),
    ]);
    const r = await h.handoffs.start("https://example.org/f.zip");
    expect(r.state).toBe("unavailable");
    expect(r.detail).toMatch(/open the Downova app, then try again/);
  });

  it("clear removes settled records and old ones expire", async () => {
    const h = harness([accepted, () => ({ kind: "timeout" })]);
    await h.handoffs.start("https://example.org/a");
    await h.handoffs.start("https://example.org/b");
    await h.handoffs.clear();
    expect(h.stored()).toHaveLength(0);
    const old: HandoffRecord = {
      id: "h_old",
      url: "https://example.org/old",
      idempotencyKey: "ext_old00000",
      state: "accepted",
      detail: "",
      jobId: "job_x",
      attempts: 1,
      createdAt: 0,
      updatedAt: 0,
    };
    expect(settleStale([old], RETENTION_MS + 1)).toHaveLength(0);
    expect(settleStale([old], RETENTION_MS)).toHaveLength(1);
  });

  it("displayed links hide query strings, fragments and credentials", () => {
    expect(displayUrl("https://cdn.example.org/a/f.zip?token=SECRET#x")).toBe(
      "https://cdn.example.org/a/f.zip?…",
    );
    expect(displayUrl("https://u:pw@h/f")).toBe("https://h/f");
    expect(displayUrl("https://h/f")).toBe("https://h/f");
    expect(displayUrl("javascript:alert(1)")).toBe("javascript: link");
    expect(displayUrl("nonsense")).toBe("(invalid link)");
  });
});

suite("interpretHello", () => {
  const ok = {
    v: 1,
    request_id: "r",
    outcome: "service_ready",
    hello: { api_version: 1, service_version: "0.1.0", backend: { id: "http" } },
  };
  it("reports a ready service", () => {
    expect(interpretHello({ kind: "response", value: ok }, "r")).toEqual({
      state: "ready",
      detail: "Downova 0.1.0 is running (backend http).",
    });
  });
  it("reports a missing host or stopped service as unavailable", () => {
    expect(
      interpretHello({ kind: "error", message: "Specified native messaging host not found." }, "r")
        .state,
    ).toBe("unavailable");
    const down = {
      v: 1,
      request_id: "r",
      outcome: "unavailable",
      error: { detail: "open Downova" },
    };
    expect(interpretHello({ kind: "response", value: down }, "r")).toEqual({
      state: "unavailable",
      detail: "Open Downova.",
    });
  });
  it("treats timeouts and mismatched replies as uncertain", () => {
    expect(interpretHello({ kind: "timeout" }, "r").state).toBe("uncertain");
    expect(interpretHello({ kind: "response", value: { ...ok, request_id: "x" } }, "r").state).toBe(
      "uncertain",
    );
    expect(interpretHello({ kind: "response", value: { ...ok, v: 2 } }, "r").state).toBe(
      "uncertain",
    );
  });
});
