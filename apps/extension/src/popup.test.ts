// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { HandoffRecord } from "./handoff.js";
import { render, renderService } from "./popup.js";

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
  expect(items[0]?.textContent).toMatch(/not necessarily started/);
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
  renderService(el, { state: "ready", detail: "Downova 0.1.0 is running (backend http)." });
  expect(el.textContent).toBe("Service ready. Downova 0.1.0 is running (backend http).");
  renderService(el, { state: "unavailable", detail: "Open the Downova app, then try again." });
  expect(el.dataset.state).toBe("unavailable");
  expect(el.textContent).toMatch(/^Service unavailable. Open the Downova app/);
});
