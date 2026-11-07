// @vitest-environment jsdom
import { describe as suite, expect, it } from "vitest";
import type { NativeHostAddAuthenticated, NativeHostRequest, SessionInput } from "@dm/contracts";
import { Handoffs, type Deps, type HandoffRecord, type SendResult } from "./handoff.js";
import {
  browserKind,
  checkScope,
  httpsOrigin,
  selectCookies,
  session,
  storeForTab,
  type BrowserCookie,
  type SignInContext,
} from "./signin.js";
import { render } from "./popup.js";
import { translator } from "./capture/i18n.js";

const SECRET = "SYNTH-EXT-COOKIE-b33";
const NOW = 1_800_000_000_000;

const cookie = (over: Partial<BrowserCookie> = {}): BrowserCookie => ({
  name: "sid",
  value: SECRET,
  domain: "files.example.org",
  hostOnly: true,
  path: "/",
  secure: true,
  session: true,
  storeId: "0",
  ...over,
});

suite("scope at the click", () => {
  it("needs an https link of the same origin (scheme, host, port) as its frame, in a normal window", () => {
    const page = "https://files.example.org/list";
    expect(checkScope("https://files.example.org/a.pdf", page, false)).toEqual({
      ok: true,
      origin: "https://files.example.org",
    });
    expect(checkScope("http://files.example.org/a.pdf", page, false)).toEqual({
      ok: false,
      code: "rejected_signin_insecure",
    });
    for (const other of [
      "https://cdn.example.org/a.pdf",
      "https://files.example.org:8443/a.pdf",
      "https://example.org/a.pdf",
    ])
      expect(checkScope(other, page, false), other).toEqual({
        ok: false,
        code: "rejected_signin_cross_origin",
      });
    expect(checkScope("https://files.example.org/a.pdf", undefined, false).ok).toBe(false);
    expect(
      checkScope("https://files.example.org/a.pdf", "http://files.example.org/", false),
    ).toEqual({ ok: false, code: "rejected_signin_cross_origin" });
    expect(checkScope("https://files.example.org/a.pdf", page, true)).toEqual({
      ok: false,
      code: "rejected_signin_private",
    });
    expect(httpsOrigin("https://h.example:443/x")).toBe("https://h.example");
    expect(httpsOrigin("https://h.example:8443/x")).toBe("https://h.example:8443");
  });

  it("uses exactly one cookie store for the tab; none or several are ambiguous", () => {
    expect(storeForTab([{ id: "0", tabIds: [1, 2] }], 2)).toBe("0");
    expect(storeForTab([{ id: "0", tabIds: [1] }], 2)).toBeNull();
    expect(
      storeForTab(
        [
          { id: "0", tabIds: [2] },
          { id: "1", tabIds: [2] },
        ],
        2,
      ),
    ).toBeNull();
    expect(storeForTab([{ id: "0", tabIds: [1] }], undefined)).toBeNull();
    expect(storeForTab([{ id: "0", tabIds: [-1] }], -1)).toBeNull();
  });
});

suite("cookie selection", () => {
  it("drops partitioned, expired, other-store and malformed cookies", () => {
    const got = selectCookies(
      [
        cookie(),
        cookie({ name: "chips", partitionKey: { topLevelSite: "https://other.example" } }),
        cookie({ name: "old", session: false, expirationDate: NOW / 1000 - 1 }),
        cookie({ name: "later", session: false, expirationDate: NOW / 1000 + 60 }),
        cookie({ name: "elsewhere", storeId: "1" }),
        cookie({ name: "bad;name" }),
        cookie({ name: "nopath", path: "x" }),
      ],
      "0",
      NOW,
    );
    expect(got?.map((c) => c.name)).toEqual(["sid", "later"]);
    expect(got?.[1]?.expires_at_ms).toBe(String(NOW + 60_000));
    expect(got?.[0]).not.toHaveProperty("expires_at_ms");
  });

  it("is null when nothing usable remains, and bounded in count and size", () => {
    expect(selectCookies([cookie({ storeId: "1" })], "0", NOW)).toBeNull();
    const many = Array.from({ length: 100 }, (_, i) => cookie({ name: `c${i}` }));
    expect(selectCookies(many, "0", NOW)?.length).toBe(64);
    const big = Array.from({ length: 10 }, (_, i) =>
      cookie({ name: `b${i}`, value: "x".repeat(4000) }),
    );
    const sum = (selectCookies(big, "0", NOW) ?? []).reduce(
      (n, c) => n + c.name.length + c.value.length + c.domain.length + c.path.length,
      0,
    );
    expect(sum).toBeLessThanOrEqual(16 * 1024);
  });

  it("names the browser and builds the session", () => {
    expect(browserKind("Mozilla/5.0 … Chrome/130.0.0.0 Safari/537.36")).toBe("chrome");
    expect(browserKind("Mozilla/5.0 … Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0")).toBe("edge");
    const ctx: SignInContext = {
      origin: "https://files.example.org",
      storeId: "0",
      browser: "chrome",
      remember: true,
    };
    const s = session(ctx, "0123456789abcdef", selectCookies([cookie()], "0", NOW) ?? []);
    expect(s).toMatchObject({ origin: ctx.origin, remember: true, profile_id: "0123456789abcdef" });
    expect(() => session(ctx, "0123456789abcdef", [])).toThrow();
  });
});

function harness(sessionAnswer: Deps["session"]) {
  let stored: HandoffRecord[] = [];
  let n = 0;
  const sent: NativeHostRequest[] = [];
  const reads: string[] = [];
  const deps: Deps = {
    load: async () => structuredClone(stored),
    save: async (r) => {
      stored = structuredClone(r);
    },
    send: async (m) => {
      sent.push(m);
      const reply: SendResult =
        sent.length === 1 && m.op === "add_authenticated" && stored[0]?.attempts === 0
          ? { kind: "timeout" }
          : {
              kind: "response",
              value: {
                v: 1,
                request_id: m.request_id,
                outcome: "accepted",
                job: { job_id: "job_1" },
              },
            };
      return reply;
    },
    now: () => 1_000,
    token: () => `tok${String(++n).padStart(13, "0")}`,
    ...(sessionAnswer
      ? {
          session: async (ctx: SignInContext, url: string) => {
            reads.push(url);
            return sessionAnswer(ctx, url);
          },
        }
      : {}),
  };
  return { handoffs: new Handoffs(deps), sent, reads, stored: () => stored };
}

const CTX: SignInContext = {
  origin: "https://files.example.org",
  storeId: "0",
  browser: "edge",
  remember: false,
};
const SESSION: SessionInput = session(CTX, "0123456789abcdef", [
  {
    name: "sid",
    value: SECRET,
    domain: "files.example.org",
    host_only: true,
    path: "/",
    secure: true,
  },
]);

suite("signed-in hand-off", () => {
  it("sends add_authenticated once per action, re-reads cookies on retry, never stores them", async () => {
    const h = harness(async () => ({ ok: true, session: SESSION }));
    const first = await h.handoffs.start("https://files.example.org/a.pdf", 50, CTX);
    expect(first.state).toBe("uncertain");
    const again = await h.handoffs.retry(first.id);
    expect(again?.state).toBe("accepted");
    expect(h.reads).toHaveLength(2);
    const msgs = h.sent as NativeHostAddAuthenticated[];
    expect(msgs.map((m) => m.op)).toEqual(["add_authenticated", "add_authenticated"]);
    expect(msgs[0]?.idempotency_key).toBe(msgs[1]?.idempotency_key);
    expect(msgs[0]?.session.cookies[0]?.value).toBe(SECRET);
    // Storage holds the record and its non-secret context only.
    const disk = JSON.stringify(h.stored());
    expect(disk).not.toContain(SECRET);
    expect(h.stored()[0]?.signIn).toEqual(CTX);
  });

  it("settles without sending when the sign-in cannot be used", async () => {
    const h = harness(async () => ({ ok: false, code: "rejected_signin_no_cookies" }));
    const r = await h.handoffs.start("https://files.example.org/a.pdf", 50, CTX);
    expect(r).toMatchObject({ state: "rejected", code: "rejected_signin_no_cookies" });
    expect(h.sent).toHaveLength(0);
  });

  it("records a refusal at the click, and refuses without a session reader", async () => {
    const h = harness(undefined);
    const refused = await h.handoffs.start(
      "https://cdn.example.org/a.pdf",
      50,
      "rejected_signin_cross_origin",
    );
    expect(refused).toMatchObject({ state: "rejected", code: "rejected_signin_cross_origin" });
    const noReader = await h.handoffs.start("https://files.example.org/a.pdf", 50, CTX);
    expect(noReader.code).toBe("rejected_signin_permission");
    expect(h.sent).toHaveLength(0);
  });

  it("the popup shows the origin and mode in both languages, never a cookie", async () => {
    const h = harness(async () => ({ ok: true, session: SESSION }));
    await h.handoffs.start("https://files.example.org/a.pdf", 50, { ...CTX, remember: true });
    for (const lang of ["en", "bn"] as const) {
      const list = document.createElement("ul");
      const empty = document.createElement("p");
      render(list, empty, h.stored(), () => undefined, translator(lang));
      const line = list.querySelector<HTMLElement>('[data-role="signin"]');
      expect(line?.dataset.mode).toBe("remembered");
      expect(line?.textContent).toContain("https://files.example.org");
      expect(list.textContent).not.toContain(SECRET);
    }
  });
});
