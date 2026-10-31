// B2.13 automatic capture: policy precedence, request evidence and the durable state machine
// against a scripted browser and native host. These are unit tests of the logic; the real
// downloads/webRequest events are exercised by the real-browser test (scripts/e2e).
import { describe, expect, it } from "vitest";
import type { NativeHostRequest } from "@dm/contracts";
import type { SendResult } from "../handoff.js";

const keyOf = (m: NativeHostRequest | undefined) =>
  m && m.op === "add" ? m.idempotency_key : null;
import {
  Captures,
  MAX_IN_FLIGHT,
  recordId,
  type BrowserItem,
  type CaptureRecord,
} from "./captures.js";
import {
  correlate,
  onBeforeRedirect,
  onHeadersReceived,
  onSendHeaders,
  type Evidence,
} from "./evidence.js";
import {
  DEFAULT_SETTINGS,
  decide,
  extensionOf,
  hostMatches,
  normalizeSite,
  originsFor,
  sanitizeSettings,
  type CaptureSettings,
  type Item,
} from "./policy.js";

const SECRET = "SYNTHETIC-SECRET-capture";

const settings = (over: Partial<CaptureSettings> = {}): CaptureSettings => ({
  ...DEFAULT_SETTINGS,
  enabled: true,
  sites: ["example.org"],
  ...over,
});

function item(over: Partial<BrowserItem> = {}): BrowserItem {
  return {
    id: 7,
    url: "https://dl.example.org/file.zip",
    finalUrl: "https://dl.example.org/file.zip",
    mime: "application/zip",
    totalBytes: 5_000_000,
    filename: "C:\\Users\\u\\Downloads\\file.zip",
    danger: "safe",
    incognito: false,
    state: "in_progress",
    startTime: "2026-10-31T10:00:00.000Z",
    ...over,
  };
}

function evidence(over: Partial<Evidence> = {}): Evidence {
  return {
    requestId: "1",
    url: "https://dl.example.org/file.zip",
    method: "GET",
    hosts: ["dl.example.org"],
    sentCookies: false,
    sentCredentials: false,
    status: 200,
    completedAt: 1_000,
    startedAt: 900,
    ...over,
  };
}

const all = () => true;
const match = (e: Evidence = evidence()) => ({ kind: "match" as const, evidence: e });

// ------------------------------------------------------------------ policy

describe("site matching", () => {
  it("respects domain boundaries", () => {
    expect(hostMatches("example.org", "example.org")).toBe(true);
    expect(hostMatches("dl.example.org", "example.org")).toBe(true);
    expect(hostMatches("badexample.org", "example.org")).toBe(false);
    expect(hostMatches("example.org.evil.net", "example.org")).toBe(false);
    expect(hostMatches("EXAMPLE.ORG.", "example.org")).toBe(true);
  });

  it("normalizes entries and drops invalid ones", () => {
    expect(normalizeSite("https://Downloads.Example.org:8443/path?q=1")).toBe(
      "downloads.example.org",
    );
    expect(normalizeSite("*.example.org")).toBe("example.org");
    expect(normalizeSite("127.0.0.1")).toBe("127.0.0.1");
    for (const bad of ["", "exa mple.org", "-.org..", "[::1]", "a".repeat(300)])
      expect(normalizeSite(bad), bad).toBeNull();
    expect(originsFor("example.org")).toContain("https://*.example.org/*");
    expect(originsFor("127.0.0.1")).toEqual(["http://127.0.0.1/*", "https://127.0.0.1/*"]);
  });

  it("treats file names as data", () => {
    expect(extensionOf("C:\\a\\b.tar.GZ")).toBe("gz");
    expect(extensionOf("/x/../..\\evil")).toBe("");
    expect(extensionOf("")).toBe("");
  });

  it("sanitizes stored settings and bounds lists", () => {
    const s = sanitizeSettings({
      enabled: "yes",
      sites: ["Example.org", "example.org", "bad host", 4],
      extensions: [".ZIP", "toolongextension", "iso"],
      mimeTypes: ["Video/*", "nonsense"],
      minSizeBytes: -5,
    });
    expect(s).toEqual({
      enabled: false,
      sites: ["example.org"],
      excluded: [],
      extensions: ["zip", "iso"],
      mimeTypes: ["video/*"],
      minSizeBytes: 0,
    });
  });
});

describe("eligibility precedence", () => {
  it("captures an ordinary public GET from an allowed site", () => {
    expect(decide(item(), match(), settings(), all)).toEqual({ eligible: true });
  });

  it("keeps unsupported schemes, credentials and incognito in the browser", () => {
    const why = (i: Item) => decide(i, match(), settings(), all);
    expect(why(item({ url: "blob:https://x/1", finalUrl: "blob:https://x/1" }))).toEqual({
      eligible: false,
      reason: "unsupported_scheme",
    });
    expect(why(item({ url: "data:text/plain,hi", finalUrl: "data:text/plain,hi" }))).toMatchObject({
      reason: "unsupported_scheme",
    });
    expect(
      why(item({ url: "https://u:p@dl.example.org/f", finalUrl: "https://u:p@dl.example.org/f" })),
    ).toMatchObject({ reason: "invalid_link" });
    expect(why(item({ incognito: true }))).toMatchObject({ reason: "incognito" });
  });

  it("exclusions win over the allowlist and are checked on the final URL too", () => {
    const s = settings({ excluded: ["private.example.org"] });
    expect(
      decide(item({ finalUrl: "https://private.example.org/file.zip" }), match(), s, all),
    ).toMatchObject({ reason: "site_excluded" });
    expect(
      decide(
        item({ url: "https://other.net/f.zip", finalUrl: "https://other.net/f.zip" }),
        match(),
        s,
        all,
      ),
    ).toMatchObject({ reason: "site_not_allowed" });
    expect(decide(item(), match(), s, () => false)).toMatchObject({
      reason: "permission_missing",
    });
  });

  it("never bypasses the browser's safety verdict or executable types", () => {
    expect(decide(item({ danger: "file" }), match(), settings(), all)).toMatchObject({
      reason: "browser_warning",
    });
    expect(
      decide(
        item({ filename: "C:\\d\\setup.EXE" }),
        match(),
        settings({ extensions: ["exe"] }),
        all,
      ),
    ).toMatchObject({ reason: "executable" });
  });

  it("needs one GET without cookies or credentials, 200, and allowed redirect hops", () => {
    const why = (e: Parameters<typeof decide>[1]) => decide(item(), e, settings(), all);
    expect(why({ kind: "none" })).toMatchObject({ reason: "no_request_evidence" });
    expect(why({ kind: "ambiguous" })).toMatchObject({ reason: "ambiguous_request" });
    expect(why(match(evidence({ method: "POST" })))).toMatchObject({ reason: "not_get" });
    expect(why(match(evidence({ sentCookies: true })))).toMatchObject({ reason: "sent_cookies" });
    expect(why(match(evidence({ sentCredentials: true })))).toMatchObject({
      reason: "sent_credentials",
    });
    expect(why(match(evidence({ status: 206 })))).toMatchObject({ reason: "unexpected_status" });
    expect(
      why(match(evidence({ hosts: ["dl.example.org", "tracker.evil.net", "dl.example.org"] }))),
    ).toMatchObject({ reason: "redirect_not_allowed" });
  });

  it("unknown size or MIME never satisfies a restrictive filter", () => {
    const s = settings({ mimeTypes: ["application/zip"], minSizeBytes: 1_000_000 });
    expect(decide(item({ mime: "" }), match(), s, all)).toMatchObject({ reason: "mime_unknown" });
    expect(decide(item({ mime: "text/html" }), match(), s, all)).toMatchObject({
      reason: "mime_filtered",
    });
    expect(decide(item({ totalBytes: -1 }), match(), s, all)).toMatchObject({
      reason: "size_unknown",
    });
    expect(decide(item({ totalBytes: 10 }), match(), s, all)).toMatchObject({
      reason: "too_small",
    });
    expect(decide(item(), match(), settings({ extensions: ["iso"] }), all)).toMatchObject({
      reason: "type_filtered",
    });
    expect(decide(item(), match(), settings({ mimeTypes: ["application/*"] }), all)).toEqual({
      eligible: true,
    });
  });
});

// ------------------------------------------------------------------ evidence

describe("request evidence", () => {
  it("records header names only and follows redirects", () => {
    let list: Evidence[] = [];
    list = onSendHeaders(
      list,
      {
        requestId: "9",
        url: "https://a.example.org/go",
        method: "GET",
        requestHeaders: [{ name: "Accept" }],
      },
      100,
    );
    list = onBeforeRedirect(list, { requestId: "9", redirectUrl: "https://cdn.example.org/f.zip" });
    list = onSendHeaders(
      list,
      {
        requestId: "9",
        url: "https://cdn.example.org/f.zip",
        method: "GET",
        requestHeaders: [{ name: "Cookie" }],
      },
      110,
    );
    list = onHeadersReceived(
      list,
      { requestId: "9", url: "https://cdn.example.org/f.zip", statusCode: 200 },
      120,
    );
    const [e] = list;
    expect(e).toMatchObject({
      hosts: ["a.example.org", "cdn.example.org"],
      sentCookies: true,
      status: 200,
    });
    expect(JSON.stringify(list)).not.toContain(SECRET);
  });

  it("matches exactly one completed observation; several are ambiguous", () => {
    const one = [evidence()];
    expect(correlate(one, item(), 1_000).kind).toBe("match");
    expect(correlate([evidence({ completedAt: 0 })], item(), 1_000).kind).toBe("pending");
    expect(correlate([], item(), 1_000).kind).toBe("none");
    expect(correlate([evidence(), evidence({ requestId: "2" })], item(), 1_000).kind).toBe(
      "ambiguous",
    );
  });
});

// ------------------------------------------------------------------ state machine

type Reply = SendResult | ((m: NativeHostRequest) => SendResult);

class Browser {
  items = new Map<number, BrowserItem>();
  cancelled: number[] = [];
  erased: number[] = [];
}

function accepted(job = "job_cap0001"): Reply {
  return (m) => ({
    kind: "response",
    value: {
      v: 1,
      request_id: m.request_id,
      outcome: "accepted",
      job: { job_id: job, phase: "queued" },
    },
  });
}

function harness(opts: { settings?: CaptureSettings; evidence?: Evidence[] } = {}) {
  let stored: CaptureRecord[] = [];
  let now = 1_000;
  let tokens = 0;
  const browser = new Browser();
  const sent: NativeHostRequest[] = [];
  const replies: Reply[] = [];
  const state = {
    settings: opts.settings ?? settings(),
    apis: true,
    hosts: true,
    evidence: opts.evidence ?? [evidence()],
  };
  const captures = () =>
    new Captures({
      load: async () => structuredClone(stored),
      save: async (r) => {
        stored = structuredClone(r);
      },
      settings: async () => state.settings,
      hasApis: async () => state.apis,
      hostPermissions: async () => () => state.hosts,
      evidence: async () => state.evidence,
      item: async (id) => {
        const found = browser.items.get(id);
        return found ? { ...found } : null;
      },
      cancel: async (id) => {
        browser.cancelled.push(id);
        const i = browser.items.get(id);
        if (i && i.state === "in_progress")
          browser.items.set(id, { ...i, state: "interrupted", error: "USER_CANCELED" });
      },
      send: async (m) => {
        sent.push(m);
        const r = replies.shift() ?? { kind: "timeout" };
        return typeof r === "function" ? r(m) : r;
      },
      now: () => now,
      token: () => `t${String(++tokens).padStart(16, "0")}`,
      sleep: async (ms) => {
        now += ms;
      },
    });
  return {
    browser,
    sent,
    replies,
    state,
    captures,
    stored: () => stored,
    advance: (ms: number) => {
      now += ms;
    },
  };
}

describe("capture state machine", () => {
  it("hands a supported download over and cancels the browser copy only after acceptance", async () => {
    const h = harness();
    const it7 = item();
    h.browser.items.set(7, it7);
    h.replies.push(accepted());
    const rec = await h.captures().onCreated(it7);
    expect(rec).toMatchObject({
      state: "accepted",
      jobId: "job_cap0001",
      browser: "cancelled_by_downova",
    });
    expect(h.sent).toHaveLength(1);
    expect(h.sent[0]).toMatchObject({ op: "add", url: it7.url });
    expect(h.browser.cancelled).toEqual([7]);
    expect(h.browser.erased).toEqual([]);
  });

  it("does nothing while capture is off, for incognito or for sites not allowed", async () => {
    for (const [over, s] of [
      [{}, settings({ enabled: false })],
      [{ incognito: true }, settings()],
      [{ url: "https://x.net/f.zip", finalUrl: "https://x.net/f.zip" }, settings()],
      [{ byExtensionId: "abc" }, settings()],
    ] as const) {
      const h = harness({ settings: s });
      const i = item(over);
      h.browser.items.set(7, i);
      expect(await h.captures().onCreated(i)).toBeNull();
      expect(h.sent).toHaveLength(0);
      expect(h.stored()).toHaveLength(0);
    }
  });

  it("POST, cookie and blob downloads stay in the browser and are never sent", async () => {
    for (const [over, ev, reason] of [
      [{}, evidence({ method: "POST" }), "not_get"],
      [{}, evidence({ sentCookies: true }), "sent_cookies"],
      [
        { url: "blob:https://dl.example.org/1", finalUrl: "blob:https://dl.example.org/1" },
        evidence(),
        null,
      ],
    ] as const) {
      const h = harness({ evidence: [ev] });
      const i = item(over);
      h.browser.items.set(7, i);
      const rec = await h.captures().onCreated(i);
      if (reason) expect(rec).toMatchObject({ state: "browser_owned", reason });
      expect(h.sent).toHaveLength(0);
      expect(h.browser.cancelled).toEqual([]);
    }
  });

  it("a duplicate onCreated for the same download never sends twice", async () => {
    const h = harness();
    const i = item();
    h.browser.items.set(7, i);
    h.replies.push(accepted());
    const c = h.captures();
    const [a, b] = await Promise.all([c.onCreated(i), c.onCreated(i)]);
    expect(h.sent).toHaveLength(1);
    expect([a?.state, b]).toEqual(["accepted", null]);
  });

  it("an unconfirmed send is retried once with the same key; still unconfirmed stays uncertain and the browser continues", async () => {
    const h = harness();
    const i = item();
    h.browser.items.set(7, i);
    h.replies.push({ kind: "timeout" }, { kind: "timeout" });
    const rec = await h.captures().onCreated(i);
    expect(rec).toMatchObject({ state: "uncertain", attempts: 2, browser: "running" });
    expect(h.sent).toHaveLength(2);
    expect(keyOf(h.sent[0])).toBe(keyOf(h.sent[1]));
    expect(h.sent[0]?.request_id).not.toBe(h.sent[1]?.request_id);
    expect(h.browser.cancelled).toEqual([]);
  });

  it("a lost acknowledgement then acceptance (same key) takes over once", async () => {
    const h = harness();
    const i = item();
    h.browser.items.set(7, i);
    h.replies.push({ kind: "timeout" }, accepted("job_same"));
    const rec = await h.captures().onCreated(i);
    expect(rec).toMatchObject({
      state: "accepted",
      jobId: "job_same",
      browser: "cancelled_by_downova",
    });
    expect(h.browser.cancelled).toEqual([7]);
  });

  it("missing host, service down and malformed replies leave the browser download running", async () => {
    const cases: [Reply, string][] = [
      [
        { kind: "error", message: "Specified native messaging host not found." },
        "downova_unavailable",
      ],
      [
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
              detail: "the service is not running",
            },
          },
        }),
        "downova_unavailable",
      ],
      [
        (m) => ({
          kind: "response",
          value: { v: 1, request_id: m.request_id, outcome: "rejected" },
        }),
        "downova_rejected",
      ],
    ];
    for (const [reply, reason] of cases) {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      h.replies.push(reply);
      const rec = await h.captures().onCreated(i);
      expect(rec).toMatchObject({ state: "failed", reason });
      expect(h.browser.cancelled).toEqual([]);
    }
    const h = harness();
    h.browser.items.set(7, item());
    h.replies.push({
      kind: "response",
      value: { v: 1, request_id: "someone-else", outcome: "accepted", job: { job_id: "job_x" } },
    });
    h.replies.push({ kind: "response", value: "garbage" });
    expect(await h.captures().onCreated(item())).toMatchObject({ state: "uncertain" });
    expect(h.browser.cancelled).toEqual([]);
  });

  it("the browser finishing or the user cancelling first is reported, never cancelled again", async () => {
    for (const [end, side] of [
      [{ state: "complete" }, "finished_in_browser"],
      [{ state: "interrupted", error: "USER_CANCELED" }, "cancelled_by_user"],
    ] as const) {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      h.replies.push((m) => {
        h.browser.items.set(7, { ...i, ...end });
        return (accepted() as (m: NativeHostRequest) => SendResult)(m);
      });
      const rec = await h.captures().onCreated(i);
      expect(rec).toMatchObject({ state: "accepted", browser: side });
      expect(h.browser.cancelled).toEqual([]);
    }
  });

  it("capture disabled or permission revoked during the hand-off keeps the browser copy", async () => {
    for (const change of ["disable", "revoke"] as const) {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      h.replies.push((m) => {
        if (change === "disable") h.state.settings = { ...h.state.settings, enabled: false };
        else h.state.apis = false;
        return (accepted() as (m: NativeHostRequest) => SendResult)(m);
      });
      const rec = await h.captures().onCreated(i);
      expect(rec).toMatchObject({
        state: "accepted",
        browser: "kept",
        reason: change === "disable" ? "capture_disabled" : "permission_revoked",
      });
      expect(h.browser.cancelled).toEqual([]);
    }
  });

  it("a worker stopped at each boundary resumes with the same key and one job", async () => {
    // Stopped after the record was written, before the decision.
    {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      h.replies.push(accepted());
      await h.captures().onCreated({ ...i, state: "in_progress" });
      expect(h.sent).toHaveLength(1);
    }
    // Stopped while sending: the restarted worker re-sends with the stored key.
    {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      const pending: CaptureRecord = {
        id: recordId(i),
        downloadId: 7,
        url: i.url,
        display: "https://dl.example.org/file.zip",
        state: "sending",
        reason: null,
        idempotencyKey: "cap_stored_key_0001",
        jobId: null,
        browser: "running",
        attempts: 1,
        createdAt: 1_000,
        updatedAt: 1_000,
      };
      const c1 = h.captures();
      await (
        c1 as unknown as {
          update: (f: (r: CaptureRecord[]) => [CaptureRecord[], null]) => Promise<null>;
        }
      ).update(() => [[pending], null]);
      h.replies.push(accepted("job_resumed"));
      await h.captures().resume();
      expect(h.sent.map(keyOf)).toEqual(["cap_stored_key_0001"]);
      expect(h.stored()[0]).toMatchObject({
        state: "accepted",
        jobId: "job_resumed",
        browser: "cancelled_by_downova",
      });
      // A duplicate onCreated after the restart changes nothing.
      expect(await h.captures().onCreated(i)).toBeNull();
      expect(h.sent).toHaveLength(1);
    }
    // Stopped after acceptance, before the takeover: the restart rechecks and cancels once.
    {
      const h = harness();
      const i = item();
      h.browser.items.set(7, i);
      const c = h.captures();
      await (
        c as unknown as {
          update: (f: (r: CaptureRecord[]) => [CaptureRecord[], null]) => Promise<null>;
        }
      ).update(() => [
        [
          {
            id: recordId(i),
            downloadId: 7,
            url: i.url,
            display: "x",
            state: "accepted",
            reason: null,
            idempotencyKey: "k",
            jobId: "job_a",
            browser: "running",
            attempts: 1,
            createdAt: 1_000,
            updatedAt: 1_000,
          },
        ],
        null,
      ]);
      await h.captures().resume();
      await h.captures().resume();
      expect(h.browser.cancelled).toEqual([7]);
      expect(h.sent).toHaveLength(0);
    }
  });

  it("Downova's own cancel is never reported as the user's, even when its event arrives first or the worker restarts", async () => {
    const h = harness();
    const i = item();
    h.browser.items.set(7, i);
    h.replies.push(accepted());
    const c = h.captures();
    // The browser reports the interruption while Downova's cancel is still in progress.
    const realCancel = h.browser.cancelled;
    const rec = await c.onCreated(i);
    await c.onChanged({ id: 7, state: "interrupted", error: "USER_CANCELED" });
    expect(rec).toMatchObject({ browser: "cancelled_by_downova" });
    expect(h.stored()[0]).toMatchObject({ browser: "cancelled_by_downova", cancelRequested: true });
    expect(realCancel).toEqual([7]);
    // A restarted worker re-checking the same record keeps the attribution.
    await h.captures().resume();
    expect(h.stored()[0]).toMatchObject({ browser: "cancelled_by_downova" });
  });

  it("a cancel the user made before acceptance stays the user's", async () => {
    const h = harness();
    const i = item();
    h.browser.items.set(7, i);
    h.replies.push((m) => {
      h.browser.items.set(7, { ...i, state: "interrupted", error: "USER_CANCELED" });
      return (accepted() as (m: NativeHostRequest) => SendResult)(m);
    });
    const c = h.captures();
    const rec = await c.onCreated(i);
    expect(rec).toMatchObject({ browser: "cancelled_by_user" });
    expect(h.browser.cancelled).toEqual([]);
  });

  it("bounds concurrent captures; the rest stay in the browser as busy", async () => {
    const h = harness();
    const c = h.captures();
    const never = new Promise<SendResult>(() => {});
    h.replies.push(
      ...Array.from({ length: MAX_IN_FLIGHT }, () => (() => never) as unknown as Reply),
    );
    const starts = [];
    for (let n = 1; n <= MAX_IN_FLIGHT + 1; n++) {
      const i = item({
        id: n,
        url: `https://dl.example.org/${n}.zip`,
        finalUrl: `https://dl.example.org/${n}.zip`,
      });
      h.browser.items.set(n, i);
      h.state.evidence = [...h.state.evidence, evidence({ requestId: `r${n}`, url: i.url })];
      starts.push(c.onCreated(i));
    }
    await new Promise((r) => setTimeout(r, 20));
    const busy = h.stored().filter((r) => r.reason === "busy");
    expect(busy).toHaveLength(1);
    expect(h.browser.cancelled).toEqual([]);
  });

  it("never stores or shows signed-query values beyond the native message", async () => {
    const h = harness({
      evidence: [evidence({ url: `https://dl.example.org/file.zip?sig=${SECRET}` })],
    });
    const i = item({
      url: `https://dl.example.org/file.zip?sig=${SECRET}`,
      finalUrl: `https://dl.example.org/file.zip?sig=${SECRET}`,
    });
    h.browser.items.set(7, i);
    h.replies.push(accepted());
    const rec = await h.captures().onCreated(i);
    expect(rec?.display).toBe("https://dl.example.org/file.zip?…");
    expect(rec?.display).not.toContain(SECRET);
  });
});
