// @vitest-environment jsdom
// B2.13 options page: the activity list says what happened in words (colour is never the only
// cue), shows actionable reasons, and never shows a signed query value; both languages are
// complete.
import { describe, expect, it } from "vitest";
import type { CaptureRecord } from "./capture/captures.js";
import { CATALOGS, KEYS, translator } from "./capture/i18n.js";
import { DEFAULT_SETTINGS } from "./capture/policy.js";
import { WELCOME_PAGE } from "./messages.js";
import {
  acceptAllSites,
  activityText,
  CAPTURE_APIS,
  renderActivity,
  restartForCapture,
  wantsWelcome,
} from "./options.js";

const SECRET = "SYNTHETIC-SECRET-options";

function record(over: Partial<CaptureRecord> = {}): CaptureRecord {
  return {
    id: "cap_1_1",
    downloadId: 1,
    url: `https://dl.example.org/f.zip?sig=${SECRET}`,
    display: "https://dl.example.org/f.zip?…",
    state: "accepted",
    reason: null,
    idempotencyKey: `cap_${SECRET}`,
    jobId: "job_1",
    browser: "cancelled_by_downova",
    attempts: 1,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

describe("activity list", () => {
  const en = translator("en");

  it("shows the outcome, the browser's side and the job, never the signed query or key", () => {
    const lines = activityText(record(), en);
    expect(lines).toEqual([
      "Accepted by Downova",
      "https://dl.example.org/f.zip?…",
      "The browser's download was cancelled; Downova continues it.",
      "Downova job job_1",
    ]);
    expect(lines.join(" ")).not.toContain(SECRET);
  });

  it("gives an actionable reason for downloads kept in the browser", () => {
    expect(
      activityText(record({ state: "browser_owned", reason: "sent_cookies", jobId: null }), en),
    ).toContain("The browser sent cookies for this download (it may need your sign-in).");
    const uncertain = activityText(record({ state: "uncertain", jobId: null }), en);
    expect(uncertain.join(" ")).toMatch(/two copies/);
  });

  it("renders states as text and stays empty-safe", () => {
    const list = document.createElement("ul");
    const empty = document.createElement("p");
    renderActivity(list, empty, [], en);
    expect(empty.hidden).toBe(false);
    renderActivity(
      list,
      empty,
      [record(), record({ id: "2", state: "failed", reason: "downova_unavailable" })],
      en,
    );
    expect(empty.hidden).toBe(true);
    expect(list.querySelectorAll("li")).toHaveLength(2);
    expect(list.textContent).toContain("Kept in the browser");
    expect(list.textContent).not.toContain(SECRET);
  });
});

describe("private windows (BRW-10, option A)", () => {
  it("says on the options page, in both languages, that private windows are never captured", () => {
    expect(translator("en")("supported3")).toMatch(/private windows\.$/);
    expect(translator("bn")("supported3")).toContain("প্রাইভেট উইন্ডো");
    expect(translator("en")("reason.incognito")).toBe("Private windows are never captured.");
  });
});

describe("capture from all sites, offered once (ADR 0040)", () => {
  it("shows the offer only on the welcome page", () => {
    expect(WELCOME_PAGE).toBe("options.html?welcome=1");
    expect(wantsWelcome("?welcome=1")).toBe(true);
    expect(wantsWelcome("")).toBe(false);
    expect(wantsWelcome("?welcome=0")).toBe(false);
  });

  it("asks once for the capture APIs and all-sites access, and turns on only when granted", async () => {
    const asked: unknown[] = [];
    const yes = await acceptAllSites(DEFAULT_SETTINGS, async (p) => {
      asked.push(p);
      return true;
    });
    expect(asked).toEqual([{ permissions: CAPTURE_APIS, origins: ["http://*/*", "https://*/*"] }]);
    expect(yes).toEqual({
      granted: true,
      next: { ...DEFAULT_SETTINGS, enabled: true, allSites: true },
    });
    // Denied, or the prompt failed: nothing changes.
    for (const request of [async () => false, async () => Promise.reject(new Error("x"))]) {
      const no = await acceptAllSites(DEFAULT_SETTINGS, request);
      expect(no).toEqual({ granted: false, next: DEFAULT_SETTINGS });
    }
  });

  it("says in both languages what is never captured and that the rules still apply", () => {
    const en = translator("en");
    const bn = translator("bn");
    expect(en("welcomeText")).toMatch(/never sign-in, form or private-window downloads/);
    expect(en("allSitesHelp")).toMatch(/still apply/);
    for (const k of [
      "allSites",
      "allSitesHelp",
      "allSitesDenied",
      "welcomeTitle",
      "welcomeText",
      "welcomeYes",
      "welcomeLater",
      "welcomeDone",
      "welcomeDenied",
    ] as const)
      expect(bn(k), k).not.toBe(en(k));
  });
});

describe("restart after a capture grant (real-browser finding 2026-11-16)", () => {
  it("remembers the page, then restarts, only when capture is on", async () => {
    const calls: string[] = [];
    const deps = {
      url: "chrome-extension://x/options.html",
      reopen: async (url: string) => {
        calls.push(`reopen ${url}`);
      },
      reload: () => {
        calls.push("reload");
      },
    };
    expect(
      await restartForCapture({ ...DEFAULT_SETTINGS, enabled: true, allSites: true }, deps),
    ).toBe(true);
    // The page is remembered BEFORE the restart, or it would be lost.
    expect(calls).toEqual(["reopen chrome-extension://x/options.html", "reload"]);
    calls.length = 0;
    expect(await restartForCapture(DEFAULT_SETTINGS, deps)).toBe(false);
    expect(calls).toEqual([]);
  });
});

describe("catalogue", () => {
  it("defines every key in English and Bangla", () => {
    for (const key of KEYS) {
      expect(typeof CATALOGS.bn[key], key).toBe("string");
      if (key !== "browser.running") expect(CATALOGS.bn[key].length, key).toBeGreaterThan(0);
    }
    expect(translator("bn")("popupOn", { count: 2 })).toContain("2");
  });
});
