// ADV12: the virtualized download list with 10,000 service-shaped records (test fixtures only;
// product builds never contain fake records). jsdom has no layout, so the list uses its fallback
// viewport (640 px) and row height (32 px): about 20 rows in view plus overscan.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";
import { OVERSCAN, visibleIndices } from "../features/downloads/DownloadTable";

const N = 10_000;
const name = (n: number) =>
  n % 7 === 0 ? `দলিল-${n}.pdf` : `file-${String(n).padStart(5, "0")}.zip`;

function history(n = N): DownloadSnapshot[] {
  return Array.from({ length: n }, (_, i) =>
    snapshot({
      job_id: `job_${(i + 1).toString(16).padStart(8, "0")}`,
      display_name: name(i),
      source_display: `https://example.org/${name(i)}`,
      phase: i % 11 === 0 ? "failed" : "completed",
      downloaded_bytes: String(1000 + i),
      total_bytes: String(1000 + i),
      queue_id: "main",
      queue_position: String(i + 1),
    }),
  );
}

/** The `n`th element, failing the test (not the type checker) when it is missing. */
function at<T>(list: readonly T[], n: number): T {
  const v = list[n];
  if (v === undefined) throw new Error(`no element ${n}`);
  return v;
}

function setup(items = history()) {
  const fake = new FakeServiceClient();
  fake.streaming = true;
  fake.items = items;
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()} reconnectDelaysMs={[20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { fake, user: userEvent.setup() };
}

const rendered = () => document.querySelectorAll("[data-job-id]");
const table = () => screen.getByRole("table");
const rowButton = (n: number) =>
  document.querySelector<HTMLButtonElement>(
    `[data-job-id="job_${(n + 1).toString(16).padStart(8, "0")}"]`,
  );

describe("virtualized download list (ADV12)", () => {
  it("keeps the DOM bounded for 10,000 records and states the full size to assistive technology", async () => {
    setup();
    await screen.findByRole("button", { name: name(1) });
    expect(table()).toHaveAttribute("aria-rowcount", String(N + 1));
    // ~20 rows in view + overscan on each side.
    expect(rendered().length).toBeLessThanOrEqual(20 + 2 * OVERSCAN + 2);
    expect(rendered().length).toBeGreaterThan(10);
    const firstRow = rowButton(0)?.closest("tr");
    expect(firstRow).toHaveAttribute("aria-rowindex", "2");
    // Spacer rows carry no content for assistive technology.
    const spacers = document.querySelectorAll("tr.download-table__spacer");
    expect(spacers.length).toBeGreaterThan(0);
    for (const s of spacers) expect(s).toHaveAttribute("aria-hidden", "true");
    // The initial history is not announced row by row.
    for (const live of document.querySelectorAll('[aria-live="polite"]'))
      expect(live.textContent ?? "").not.toMatch(/file-0/);
  });

  it("End, Page Up and Home reach offscreen rows, select them and move focus there", async () => {
    const { user } = setup();
    const first = await screen.findByRole("button", { name: name(0) });
    first.focus();
    await user.keyboard("{End}");
    await waitFor(() => expect(rowButton(N - 1)).toHaveFocus());
    expect(rowButton(N - 1)).toHaveAttribute("aria-pressed", "true");
    expect(rowButton(N - 1)?.closest("tr")).toHaveAttribute("aria-rowindex", String(N + 1));
    await user.keyboard("{PageUp}");
    await waitFor(() => expect(document.activeElement?.getAttribute("data-job-id")).toBeTruthy());
    const afterPage = Number(document.activeElement?.closest("tr")?.getAttribute("aria-rowindex"));
    expect(afterPage).toBeLessThan(N + 1);
    expect(afterPage).toBeGreaterThan(N - 40);
    await user.keyboard("{Home}");
    await waitFor(() => expect(rowButton(0)).toHaveFocus());
    expect(rendered().length).toBeLessThanOrEqual(20 + 2 * OVERSCAN + 2);
  });

  it("searches and sorts over the full dataset, not only the rendered rows", async () => {
    const { user } = setup();
    await screen.findByRole("button", { name: name(0) });
    await user.type(screen.getByRole("searchbox"), "file-09876");
    expect(await screen.findByRole("button", { name: "file-09876.zip" })).toBeVisible();
    await user.clear(screen.getByRole("searchbox"));
    // Descending by name: the alphabetically last name of all 10,000 comes first.
    const sort = screen.getByRole("button", { name: "File name" });
    await user.click(sort);
    await user.click(sort);
    const last = [...history()]
      .map((d) => d.display_name)
      .sort(new Intl.Collator("en", { numeric: true, sensitivity: "base" }).compare)
      .at(-1);
    await waitFor(() => expect(rendered()[0]?.textContent).toBe(last));
  });

  it("progress of offscreen and visible rows changes neither the rendered window nor the focus", async () => {
    const { fake } = setup(
      history(200).map((d, i) =>
        i < 3 ? { ...d, phase: "transferring", downloaded_bytes: "0", total_bytes: "1000" } : d,
      ),
    );
    const first = await screen.findByRole("button", { name: name(0) });
    first.focus();
    const before = rendered().length;
    act(() => {
      fake.emit("progress", { ...at(fake.items, 150), downloaded_bytes: "1100" });
      fake.emit("progress", { ...at(fake.items, 0), downloaded_bytes: "500" });
    });
    await waitFor(() =>
      expect(
        within(first.closest("tr") ?? document.body).getByText("Downloading 50.0%"),
      ).toBeVisible(),
    );
    expect(rendered().length).toBe(before);
    expect(first).toHaveFocus();
  });

  it("a removed row leaves the list without breaking the window, and cannot come back", async () => {
    const { fake } = setup(history(300));
    await screen.findByRole("button", { name: name(0) });
    const gone = at(fake.items, 1);
    act(() => {
      fake.emit("job_removed", { ...gone, removed: true });
    });
    await waitFor(() => expect(table()).toHaveAttribute("aria-rowcount", "300"));
    act(() => {
      fake.emit("progress", { ...gone, downloaded_bytes: "1" });
    });
    await new Promise((r) => setTimeout(r, 120));
    expect(table()).toHaveAttribute("aria-rowcount", "300");
    expect(screen.queryByRole("button", { name: gone.display_name })).toBeNull();
  });

  it("reorders a download reached beyond the initial viewport (Alt+Up in its queue)", async () => {
    const items = history(500).map((d) => ({
      ...d,
      phase: "queued" as const,
      downloaded_bytes: "0",
    }));
    const { fake, user } = setup(items);
    const first = await screen.findByRole("button", { name: name(0) });
    first.focus();
    await user.keyboard("{End}");
    await waitFor(() => expect(rowButton(499)).toHaveFocus());
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    await waitFor(() => expect(fake.count("move")).toBe(1));
    expect(fake.calls.find((c) => c.method === "move")?.args).toEqual([
      at(items, 499).job_id,
      "up",
    ]);
  });
});

describe("virtualized list: selection, focus and removal (B2.24)", () => {
  const scroller = () => {
    const el = document.querySelector<HTMLElement>(".download-table__scroll");
    if (!el) throw new Error("no scroller");
    return el;
  };
  const scrollTo = (top: number) =>
    act(() => {
      scroller().scrollTop = top;
      scroller().dispatchEvent(new Event("scroll"));
    });

  it("removing a selected row scrolled out of view selects its true neighbour, not the next rendered row", async () => {
    const { user } = setup(history(3000));
    await user.click(await screen.findByRole("button", { name: name(2) }));
    scrollTo(32 * 2500);
    await waitFor(() => expect(rowButton(2500)).not.toBeNull());
    // The selected row stays rendered (pinned) far from the rendered window.
    expect(rowButton(2)).not.toBeNull();
    expect(rowButton(3)).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: "File" }));
    await user.click(screen.getByRole("menuitem", { name: "Remove from list…" }));
    const dialog = screen.getByRole("dialog", { name: "Remove this download from the list?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove from list" }));
    await waitFor(() => expect(rowButton(3)).toHaveFocus());
    expect(rowButton(3)).toHaveAttribute("aria-pressed", "true");
    expect(table()).toHaveAttribute("aria-rowcount", "3000");
  });

  it("keeps focus in the list when the focused row is removed by someone else", async () => {
    const { fake, user } = setup(history(400));
    const first = await screen.findByRole("button", { name: name(0) });
    first.focus();
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
    await waitFor(() => expect(rowButton(3)).toHaveFocus());
    act(() => {
      fake.emit("job_removed", { ...at(fake.items, 3), removed: true });
    });
    await waitFor(() => expect(rowButton(4)).toHaveFocus());
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(rowButton(5)).toHaveFocus());
  });

  it("keeps the selection through sorting and filtering, and the row counts follow the filter", async () => {
    const { user } = setup(history(2000));
    const first = await screen.findByRole("button", { name: name(0) });
    first.focus();
    await user.keyboard("{End}");
    await waitFor(() => expect(rowButton(1999)).toHaveFocus());
    const picked = rowButton(1999);
    // Sort by name, descending and ascending: the selected row stays selected and rendered.
    const nameHeader = screen.getByRole("button", { name: "File name" });
    await user.click(nameHeader);
    await user.click(nameHeader);
    expect(rowButton(1999)).toHaveAttribute("aria-pressed", "true");
    // A search that excludes it: the counts describe the filtered list only.
    await user.type(screen.getByRole("searchbox"), "দলিল");
    await waitFor(() =>
      expect(table()).toHaveAttribute("aria-rowcount", String(Math.ceil(2000 / 7) + 1)),
    );
    // Clearing the search brings it back, still selected.
    await user.clear(screen.getByRole("searchbox"));
    await waitFor(() => expect(table()).toHaveAttribute("aria-rowcount", "2001"));
    expect(picked).toBeTruthy();
    expect(rowButton(1999)).toHaveAttribute("aria-pressed", "true");
  });

  it("spacer rows are hidden from assistive technology and row indexes stay continuous", async () => {
    setup(history(1000));
    await screen.findByRole("button", { name: name(0) });
    scrollTo(32 * 500);
    await waitFor(() => expect(rowButton(500)).not.toBeNull());
    const rows = Array.from(document.querySelectorAll("tbody tr"));
    for (const r of rows.filter((r) => !r.hasAttribute("aria-rowindex")))
      expect(r).toHaveAttribute("aria-hidden", "true");
    const indexes = rows
      .filter((r) => r.hasAttribute("aria-rowindex"))
      .map((r) => Number(r.getAttribute("aria-rowindex")));
    for (let i = 1; i < indexes.length; i++)
      expect(at(indexes, i)).toBeGreaterThan(at(indexes, i - 1));
    expect(Math.max(...indexes)).toBeLessThanOrEqual(1001);
  });
});

describe("visibleIndices", () => {
  it("renders the viewport plus overscan and always the pinned rows", () => {
    const shown = visibleIndices(10_000, 32 * 5_000, 640, 32, [3, 9_999]);
    expect(shown).toContain(3);
    expect(shown).toContain(9_999);
    expect(shown).toContain(5_000);
    expect(shown.length).toBeLessThanOrEqual(20 + 2 * OVERSCAN + 1 + 2);
    expect(visibleIndices(0, 0, 640, 32, [0])).toEqual([]);
  });
});
