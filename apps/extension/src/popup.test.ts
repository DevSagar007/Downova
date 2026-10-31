// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { HandoffRecord } from "./handoff.js";
import { localizePage, render, renderService } from "./popup.js";
import { CATALOGS, translator } from "./capture/i18n.js";

const base: HandoffRecord = {
  id: "h_1",
  url: "https://example.org/f.zip?token=SECRET",
  idempotencyKey: "ext_1",
  state: "accepted",
  detail: "",
  jobId: "job_1",
  attempts: 1,
  createdAt: 0,
  updatedAt: 0,
};

it("shows every outcome as text, with retry only where it is safe", () => {
  const list = document.createElement("ul");
  const empty = document.createElement("p");
  const act = vi.fn();
  render(
    list,
    empty,
    [
      base,
      { ...base, id: "h_2", state: "uncertain", jobId: null },
      { ...base, id: "h_3", state: "unavailable", jobId: null },
      { ...base, id: "h_4", state: "rejected", jobId: null, detail: "Only http and https links." },
    ],
    act,
  );
  expect(empty.hidden).toBe(true);
  const items = [...list.querySelectorAll("li")];
  expect(items.map((li) => li.querySelector(".state")?.textContent)).toEqual([
    "Accepted by Downova",
    "Not confirmed",
    "Downova unavailable",
    "Not accepted",
  ]);
  expect(items[0]?.textContent).toMatch(/does not yet mean the transfer started or finished/);
  const retries = [...list.querySelectorAll("button")].filter((b) => b.textContent === "Retry");
  expect(retries).toHaveLength(2);
  expect(retries[0]?.getAttribute("aria-label")).toBe(
    "Retry hand-off of https://example.org/f.zip?…",
  );
  retries[0]?.click();
  expect(act).toHaveBeenCalledWith({ type: "retry", id: "h_2" });
});

it("shows the empty hint when nothing was handed off", () => {
  const list = document.createElement("ul");
  const empty = document.createElement("p");
  render(list, empty, [], () => undefined);
  expect(empty.hidden).toBe(false);
  expect(list.children).toHaveLength(0);
});

it("never shows a link's query string", () => {
  const list = document.createElement("ul");
  render(list, document.createElement("p"), [{ ...base, state: "uncertain" }], () => undefined);
  expect(list.textContent).not.toContain("SECRET");
  for (const b of list.querySelectorAll("button"))
    expect(b.getAttribute("aria-label")).not.toContain("SECRET");
});

it("shows the service readiness as text", () => {
  const el = document.createElement("p");
  renderService(el, {
    state: "ready",
    code: "ready",
    detail: "",
    version: "0.1.0",
    backend: "http",
  });
  expect(el.textContent).toBe("Service ready. Downova 0.1.0 is running (backend http).");
  renderService(el, { state: "unavailable", code: "unavailable", detail: "SERVICE_DETAIL" });
  expect(el.dataset.state).toBe("unavailable");
  expect(el.textContent).toMatch(/^Service unavailable. Open the Downova app/);
  // The helper's English detail is never the explanation.
  expect(el.textContent).not.toContain("SERVICE_DETAIL");
  renderService(el, { state: "checking" }, translator("bn"));
  expect(el.textContent).toBe("Downova চালু আছে কিনা যাচাই চলছে…");
});

it("renders every state, action and accessible name in Bangla without losing keys or redaction", () => {
  const list = document.createElement("ul");
  const empty = document.createElement("p");
  const act = vi.fn();
  const records = [
    base,
    { ...base, id: "h_2", state: "uncertain" as const, code: "uncertain" as const, jobId: null },
    {
      ...base,
      id: "h_3",
      state: "unavailable" as const,
      code: "host_missing" as const,
      jobId: null,
    },
    {
      ...base,
      id: "h_4",
      state: "rejected" as const,
      code: "rejected_scheme" as const,
      jobId: null,
    },
  ];
  render(list, empty, records, act, translator("bn"));
  const items = [...list.querySelectorAll("li")];
  expect(items.map((li) => li.querySelector(".state")?.textContent)).toEqual([
    "Downova গ্রহণ করেছে",
    "নিশ্চিত হয়নি",
    "Downova পাওয়া যাচ্ছে না",
    "গ্রহণ করা হয়নি",
  ]);
  expect(items[0]?.textContent).toContain("Downova জব job_1");
  expect(items[0]?.textContent).toMatch(/স্থানান্তর শুরু বা শেষ হওয়া নয়/);
  expect(items[2]?.textContent).toMatch(/ব্রাউজার সহায়ক ইনস্টল নেই/);
  const retry = [...list.querySelectorAll("button")].find((b) => b.textContent === "আবার চেষ্টা");
  expect(retry?.getAttribute("aria-label")).toBe("https://example.org/f.zip?… আবার পাঠান");
  expect(list.innerHTML).not.toContain("SECRET");
  // Changing the language only re-renders: nothing is sent or retried.
  expect(act).not.toHaveBeenCalled();
  retry?.click();
  expect(act).toHaveBeenCalledWith({ type: "retry", id: "h_2" });
});

it("renders records stored before codes existed from their state, keeping their keys", () => {
  const list = document.createElement("ul");
  const old = { ...base, id: "h_old", state: "uncertain" as const, jobId: null };
  delete (old as { code?: string }).code;
  const act = vi.fn();
  render(list, document.createElement("p"), [old], act);
  expect(list.textContent).toMatch(/No confirmation arrived/);
  list.querySelector("button")?.click();
  expect(act).toHaveBeenCalledWith({ type: "retry", id: "h_old" });
});

it("localizes the static popup text and the page language", () => {
  document.body.innerHTML = `<h2 data-i18n="popup.recentHeading">Recent hand-offs</h2>
    <button data-i18n="popup.check">Check again</button>`;
  localizePage(document, translator("bn"), "bn");
  expect(document.documentElement.lang).toBe("bn");
  expect(document.querySelector("h2")?.textContent).toBe(CATALOGS.bn["popup.recentHeading"]);
  expect(document.querySelector("button")?.textContent).toBe("আবার যাচাই করুন");
});
