// @vitest-environment jsdom
// B2.13 options page: the activity list says what happened in words (colour is never the only
// cue), shows actionable reasons, and never shows a signed query value; both languages are
// complete.
import { describe, expect, it } from "vitest";
import type { CaptureRecord } from "./capture/captures.js";
import { CATALOGS, KEYS, translator } from "./capture/i18n.js";
import { activityText, renderActivity } from "./options.js";

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

describe("catalogue", () => {
  it("defines every key in English and Bangla", () => {
    for (const key of KEYS) {
      expect(typeof CATALOGS.bn[key], key).toBe("string");
      if (key !== "browser.running") expect(CATALOGS.bn[key].length, key).toBeGreaterThan(0);
    }
    expect(translator("bn")("popupOn", { count: 2 })).toContain("2");
  });
});
