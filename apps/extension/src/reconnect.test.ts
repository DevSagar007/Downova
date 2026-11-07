// @vitest-environment jsdom
import { describe as suite, expect, it } from "vitest";
import type {
  NativeHostReconnect,
  NativeHostRequest,
  ReconnectCandidate,
  SessionInput,
} from "@dm/contracts";
import type { SendResult } from "./handoff.js";
import { Reconnects, type ReconnectDeps, type ReconnectRecord } from "./reconnect.js";
import { render, statusKey } from "./reconnect-page.js";
import { checkPage, checkScope, hostCookies, type BrowserCookie } from "./signin.js";
import { translator } from "./capture/i18n.js";

const SECRET = "SYNTH-EXT-RECONNECT-b33";
const ORIGIN = "https://files.example.org";

const candidate = (id: string, over: Partial<ReconnectCandidate> = {}): ReconnectCandidate => ({
  job_id: id,
  display_name: `${id}.pdf`,
  phase: "queued",
  desired_state: "running",
  downloaded_bytes: "1048576",
  total_bytes: "4194304",
  auth: {
    credential_id: "cred_0123456789abcdef",
    origin: ORIGIN,
    browser: "chrome",
    mode: "temporary",
    state: "missing",
  },
  ...over,
});

const SESSION: SessionInput = {
  origin: ORIGIN,
  browser: "chrome",
  profile_id: "0123456789abcdef",
  cookies: [
    {
      name: "sid",
      value: SECRET,
      domain: "files.example.org",
      host_only: true,
      path: "/",
      secure: true,
    },
  ],
  remember: false,
};

function harness(reply: (m: NativeHostRequest, n: number) => SendResult) {
  let stored: ReconnectRecord[] = [];
  let n = 0;
  const sent: NativeHostRequest[] = [];
  let reads = 0;
  const deps: ReconnectDeps = {
    load: async () => structuredClone(stored),
    save: async (r) => {
      stored = structuredClone(r);
    },
    send: async (m) => {
      sent.push(m);
      return reply(m, sent.length);
    },
    session: async () => {
      reads += 1;
      return { ok: true, session: SESSION };
    },
    profileId: async () => "0123456789abcdef",
    now: () => 1_000,
    token: () => `tok${String(++n).padStart(13, "0")}`,
  };
  return {
    r: new Reconnects(deps),
    sent,
    stored: () => stored,
    reads: () => reads,
  };
}

const accepted = (m: NativeHostRequest, extra: object = {}): SendResult => ({
  kind: "response",
  value: { v: 1, request_id: m.request_id, outcome: "accepted", ...extra },
});

suite("page and partition rules", () => {
  it("refuses a frame of another site (partitioned state), private windows and http pages", () => {
    expect(checkPage({ pageUrl: `${ORIGIN}/list` }, false)).toEqual({ ok: true, origin: ORIGIN });
    expect(checkPage({ pageUrl: `${ORIGIN}/list`, frameUrl: `${ORIGIN}/frame` }, false).ok).toBe(
      true,
    );
    expect(
      checkPage({ pageUrl: "https://news.example.com/", frameUrl: `${ORIGIN}/embed` }, false),
    ).toEqual({ ok: false, code: "rejected_signin_partitioned" });
    expect(checkPage({ pageUrl: "http://files.example.org/" }, false).ok).toBe(false);
    expect(checkPage({ pageUrl: `${ORIGIN}/` }, true)).toEqual({
      ok: false,
      code: "rejected_signin_private",
    });
    // A link inside a third-party frame is refused, even when it matches the frame.
    expect(
      checkScope(
        `${ORIGIN}/a.pdf`,
        { pageUrl: "https://news.example.com/", frameUrl: `${ORIGIN}/embed` },
        false,
      ),
    ).toEqual({ ok: false, code: "rejected_signin_partitioned" });
  });

  it("offers only cookies that apply to the origin's host", () => {
    const c = (over: Partial<BrowserCookie>): BrowserCookie => ({
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
    const picked = hostCookies(ORIGIN, [
      [c({}), c({ name: "parent", domain: ".example.org", hostOnly: false })],
      [
        c({}),
        c({ name: "deep", path: "/reports" }),
        c({ name: "sub", domain: "cdn.files.example.org" }),
      ],
    ]);
    expect(picked.map((x) => x.name)).toEqual(["sid", "parent", "deep"]);
  });
});

suite("reconnect requests", () => {
  it("lists, needs an explicit choice, sends once, and a retry reuses the operation id", async () => {
    const h = harness((m, n) => {
      if (m.op === "reconnect_candidates")
        return accepted(m, { candidates: [candidate("job_a"), candidate("job_b")] });
      // The first reconnect loses its acknowledgement.
      return n === 2
        ? { kind: "timeout" }
        : accepted(m, { job: { job_id: "job_b", phase: "queued" } });
    });
    const listed = await h.r.begin(ORIGIN, "0", "chrome");
    expect(listed.state).toBe("choosing");
    expect(listed.candidates.map((c) => c.job_id)).toEqual(["job_a", "job_b"]);
    expect(listed.jobId).toBeUndefined();
    // Unknown jobs are never sent.
    expect(await h.r.submit(listed.id, "job_zzz", false)).toBeNull();
    const first = await h.r.submit(listed.id, "job_b", true);
    expect(first?.state).toBe("uncertain");
    const again = await h.r.submit(listed.id, "job_b", true);
    expect(again?.state).toBe("reconnected");
    const sent = h.sent.filter((m) => m.op === "reconnect") as NativeHostReconnect[];
    expect(sent).toHaveLength(2);
    expect(sent[0]?.operation_id).toBe(sent[1]?.operation_id);
    expect(sent[0]).toMatchObject({
      job_id: "job_b",
      expected_credential_id: "cred_0123456789abcdef",
      resume: true,
    });
    expect(h.reads()).toBe(2);
    // Never stored, neither in the record nor anywhere else the module writes.
    expect(JSON.stringify(h.stored())).not.toContain(SECRET);
  });

  it("another download is a new operation; a request while sending is ignored", async () => {
    const h = harness((m) =>
      m.op === "reconnect_candidates"
        ? accepted(m, { candidates: [candidate("job_a"), candidate("job_b")] })
        : { kind: "error", message: "refused by test" },
    );
    const listed = await h.r.begin(ORIGIN, "0", "chrome");
    const a = await h.r.submit(listed.id, "job_a", false);
    expect(a?.state).toBe("uncertain");
    const b = await h.r.submit(listed.id, "job_b", false);
    const ops = (h.sent.filter((m) => m.op === "reconnect") as NativeHostReconnect[]).map(
      (m) => m.operation_id,
    );
    expect(new Set(ops).size).toBe(2);
    expect(b?.jobId).toBe("job_b");
  });

  it("maps refusals and an empty list", async () => {
    const none = harness((m) => accepted(m, { candidates: [] }));
    expect((await none.r.begin(ORIGIN, "0", "chrome")).state).toBe("none");
    const refused = harness((m) =>
      m.op === "reconnect_candidates"
        ? accepted(m, { candidates: [candidate("job_a")] })
        : {
            kind: "response",
            value: {
              v: 1,
              request_id: m.request_id,
              outcome: "rejected",
              error: { kind: "AUTH_SCOPE_UNSUPPORTED", retryable: false, next_action: "none" },
            },
          },
    );
    const l = await refused.r.begin(ORIGIN, "0", "chrome");
    const r = await refused.r.submit(l.id, "job_a", false);
    expect(r).toMatchObject({ state: "refused", code: "AUTH_SCOPE_UNSUPPORTED" });
    expect(statusKey(r as ReconnectRecord)).toBe("reconnect.refused.scope");
  });
});

suite("reconnect page", () => {
  function page() {
    document.body.innerHTML = `<p id="site"></p><form id="choose"><div id="candidates"></div>
      <button id="reconnect" type="submit"></button><button id="reconnect-resume" type="button"></button>
      </form><p id="status"></p>`;
  }
  const record = (over: Partial<ReconnectRecord> = {}): ReconnectRecord => ({
    id: "r_1",
    origin: ORIGIN,
    storeId: "0",
    browser: "chrome",
    state: "choosing",
    candidates: [candidate("job_a"), candidate("job_b", { desired_state: "paused" })],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  it("nothing is preselected; the buttons wait for a choice; one submit per click burst", () => {
    page();
    const calls: [string, boolean][] = [];
    render(document, record(), (j, r) => calls.push([j, r]));
    const go = document.getElementById("reconnect") as HTMLButtonElement;
    const resume = document.getElementById("reconnect-resume") as HTMLButtonElement;
    expect(go.disabled && resume.disabled).toBe(true);
    const radios = [...document.querySelectorAll<HTMLInputElement>("input[name=job]")];
    expect(radios.some((r) => r.checked)).toBe(false);
    radios[1]?.click();
    expect(go.disabled).toBe(false);
    resume.click();
    expect(calls).toEqual([["job_b", true]]);
    // While sending, nothing can be submitted again.
    render(document, record({ state: "sending", jobId: "job_b" }), (j, r) => calls.push([j, r]));
    expect((document.getElementById("reconnect") as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows outcomes in English and Bangla, never a cookie", () => {
    for (const lang of ["en", "bn"] as const) {
      page();
      const t = translator(lang);
      render(document, record({ state: "reconnected", phase: "queued" }), () => {}, t);
      const status = document.getElementById("status")?.textContent ?? "";
      expect(status).toBe(t("reconnect.state.reconnected"));
      expect(document.body.textContent).toContain(ORIGIN);
      expect(document.body.textContent).not.toContain(SECRET);
    }
    page();
    render(document, record({ state: "reconnected", phase: "failed" }), () => {});
    expect(document.getElementById("status")?.textContent).toMatch(/Retry/);
  });
});
