// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import type { HandoffRecord } from "./handoff.js";
import { render } from "./popup.js";

const base: HandoffRecord = {
  id: "h_1",
  url: "https://example.org/f.zip",
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
    "Retry hand-off of https://example.org/f.zip",
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
