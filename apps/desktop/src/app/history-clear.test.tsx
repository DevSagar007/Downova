// B2.7: CORE-10 history search and Clear finished.
import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, deferred, FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";

function renderLive(fake: FakeServiceClient, pollIntervalMs = 20) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()} pollIntervalMs={pollIntervalMs}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

const searchBox = () => screen.getByRole("searchbox", { name: "Search downloads" });
async function fileMenu(user: ReturnType<typeof userEvent.setup>, item: string) {
  await user.click(screen.getByRole("menuitem", { name: "File" }));
  return screen.getByRole("menuitem", { name: item });
}

describe("history search", () => {
  it("filters by name and source site, combines with the category, and tells no matches apart", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_a",
        display_name: "Alpha Report.pdf",
        source_display: "https://files.example.org/private/alpha?token=hidden",
        phase: "completed",
      }),
      snapshot({
        job_id: "job_b",
        display_name: "বাংলা-নোট.pdf",
        source_display: "https://bangla.example.net/one",
      }),
      snapshot({
        job_id: "job_c",
        display_name: "تقرير.pdf",
        source_display: "https://rtl.example.net/two",
        phase: "failed",
      }),
      snapshot({
        job_id: "job_d",
        display_name: "movie.mp4",
        source_display: "https://video.example.net/trailer",
      }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "Alpha Report.pdf" });

    await user.type(searchBox(), "alpha");
    expect(screen.getByRole("button", { name: "Alpha Report.pdf" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "বাংলা-নোট.pdf" })).toBeNull();
    expect(await screen.findByText("1 downloads match your search.")).toBeInTheDocument();

    await user.clear(searchBox());
    await user.type(searchBox(), "example.net");
    expect(screen.getByRole("button", { name: "বাংলা-নোট.pdf" })).toBeVisible();
    expect(screen.getByRole("button", { name: "تقرير.pdf" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Alpha Report.pdf" })).toBeNull();

    // URL path and query values are not searchable, even though the name is.
    await user.clear(searchBox());
    await user.type(searchBox(), "private");
    expect(
      within(screen.getByRole("table")).getByText("No downloads match your search."),
    ).toBeVisible();
    expect(await screen.findByText("No downloads match your search.")).toBeInTheDocument();

    // The category filter still applies before the search.
    await user.clear(searchBox());
    await user.click(screen.getByRole("button", { name: "Video" }));
    await user.type(searchBox(), "alpha");
    expect(
      within(screen.getByRole("table")).getByText("No downloads match your search."),
    ).toBeVisible();
    await user.clear(searchBox());
    expect(screen.getByRole("button", { name: "movie.mp4" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Alpha Report.pdf" })).toBeNull();
  });

  it("keeps the table's local sorting inside the filtered rows", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_a",
        display_name: "alpha.bin",
        source_display: "https://a.example.net/",
      }),
      snapshot({
        job_id: "job_b",
        display_name: "bravo.bin",
        source_display: "https://b.other.net/",
      }),
      snapshot({
        job_id: "job_c",
        display_name: "charlie.bin",
        source_display: "https://c.example.net/",
      }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "alpha.bin" });
    await user.type(searchBox(), "example.net");
    await user.click(screen.getByRole("button", { name: "File name" }));
    expect(screen.getAllByRole("rowheader").map((row) => row.textContent)).toEqual([
      "alpha.bin",
      "charlie.bin",
    ]);
    await user.click(screen.getByRole("button", { name: "File name" }));
    expect(screen.getAllByRole("rowheader").map((row) => row.textContent)).toEqual([
      "charlie.bin",
      "alpha.bin",
    ]);
    expect(screen.queryByRole("button", { name: "bravo.bin" })).toBeNull();
  });

  it("does not call an empty history a search with no matches", async () => {
    const fake = new FakeServiceClient();
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByText("Connected to download service");
    await user.type(searchBox(), "anything");
    expect(screen.getByText("No downloads in this category.")).toBeVisible();
    expect(screen.queryByText("No downloads match your search.")).toBeNull();
  });

  it("offers Ctrl/Cmd+F, clears on Escape, and restores a selection that the filter hid", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_run", display_name: "running.bin" }),
      snapshot({ job_id: "job_done", display_name: "finished.zip", phase: "completed" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    const running = await screen.findByRole("button", { name: "running.bin" });
    await user.click(running);
    expect(running).toHaveAttribute("aria-pressed", "true");

    await user.keyboard("{Control>}f{/Control}");
    expect(searchBox()).toHaveFocus();
    await user.keyboard("finished");
    expect(screen.queryByRole("button", { name: "running.bin" })).toBeNull();

    // A hidden selection never enables a row command.
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toHaveAttribute("aria-disabled", "true");
    expect(stop).toHaveAccessibleDescription(/select a download/i);

    // Escape in the field restores the full list and the still-selected row.
    searchBox().focus();
    await user.keyboard("{Escape}");
    expect(searchBox()).toHaveValue("");
    expect(screen.getByRole("button", { name: "running.bin" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Stop" })).not.toHaveAttribute("aria-disabled");
  });
});

describe("Clear finished", () => {
  it("asks once for the frozen, filter-independent scope and keeps unfinished jobs", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_a", display_name: "a.zip", phase: "completed" }),
      snapshot({ job_id: "job_b", display_name: "b.zip", phase: "failed" }),
      snapshot({ job_id: "job_c", display_name: "c.zip", phase: "cancelled" }),
      snapshot({ job_id: "job_run", display_name: "run.bin" }),
      snapshot({ job_id: "job_pause", display_name: "pause.bin", desired_state: "paused" }),
      snapshot({ job_id: "job_fin", display_name: "fin.bin", phase: "finalizing" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.zip" });
    await user.click(screen.getByRole("button", { name: "Finished" }));
    await user.click(screen.getByRole("button", { name: "a.zip" }));

    const item = await fileMenu(user, "Clear finished…");
    await user.click(item);
    let dialog = screen.getByRole("dialog", { name: "Clear finished downloads?" });
    expect(within(dialog).getByText(/3 completed, failed and cancelled downloads/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Keep them" }));
    expect(fake.count("remove")).toBe(0);

    await user.click(await fileMenu(user, "Clear finished…"));
    dialog = screen.getByRole("dialog", { name: "Clear finished downloads?" });
    expect(within(dialog).getByText(/3 completed, failed and cancelled downloads/)).toBeVisible();

    // A job that finishes while the confirmation is open is not added to its frozen scope.
    fake.items = [
      ...fake.items,
      snapshot({ job_id: "job_late", display_name: "late.zip", phase: "completed" }),
    ];
    await waitFor(() => expect(screen.getByRole("button", { name: "late.zip" })).toBeVisible());
    expect(within(dialog).getByText(/3 completed, failed and cancelled downloads/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Clear finished" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const calls = fake.calls.filter((c) => c.method === "remove").map((c) => c.args[0]);
    expect(calls.sort()).toEqual(["job_a", "job_b", "job_c"]);
    for (const name of ["a.zip", "b.zip", "c.zip"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    expect(screen.getByRole("button", { name: "late.zip" })).toBeVisible();
    expect(screen.getByRole("button", { name: "late.zip" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(fake.items.some((d) => d.job_id === "job_run")).toBe(true);
    expect(fake.items.some((d) => d.job_id === "job_pause")).toBe(true);
    expect(fake.items.some((d) => d.job_id === "job_fin")).toBe(true);
    expect(
      screen.getByText("3 finished downloads cleared from the list. Files were kept."),
    ).toBeInTheDocument();
  });

  it("prevents a second confirmation and reports partial failures with actionable errors", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_a", display_name: "a.zip", phase: "completed" }),
      snapshot({ job_id: "job_b", display_name: "b.zip", phase: "completed" }),
      snapshot({ job_id: "job_c", display_name: "c.zip", phase: "completed" }),
    ];
    fake.next("remove", async () => {
      fake.items = fake.items.filter((d) => d.job_id !== "job_a");
      return snapshot({
        job_id: "job_a",
        display_name: "a.zip",
        phase: "completed",
        removed: true,
      });
    });
    fake.next("remove", async () => {
      throw callError("rejected", "INTERNAL", "retry");
    });
    fake.next("remove", async () => {
      fake.items = fake.items.filter((d) => d.job_id !== "job_c");
      return snapshot({
        job_id: "job_c",
        display_name: "c.zip",
        phase: "completed",
        removed: true,
      });
    });
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.zip" });
    await user.click(await fileMenu(user, "Clear finished…"));
    const dialog = screen.getByRole("dialog", { name: "Clear finished downloads?" });
    const confirm = within(dialog).getByRole("button", { name: "Clear finished" });
    await user.click(confirm);
    await user.click(confirm);

    expect(await within(dialog).findByText("2 of 3 cleared; 1 kept.")).toBeVisible();
    expect(within(dialog).getByText("Some downloads were not cleared")).toBeVisible();
    expect(within(dialog).getByText(/b\.zip/)).toBeVisible();
    expect(
      within(dialog).getByText(/Downova ran into an unexpected problem\. Try again in a moment\./),
    ).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Clear finished" })).toBeNull();
    expect(fake.count("remove")).toBe(3);
    expect(screen.getByRole("button", { name: "b.zip" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "a.zip" })).toBeNull();
    expect(screen.queryByRole("button", { name: "c.zip" })).toBeNull();
  });

  it("keeps every row and explains an unreachable service", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_a", display_name: "a.zip", phase: "completed" }),
      snapshot({ job_id: "job_b", display_name: "b.zip", phase: "failed" }),
    ];
    fake.next("remove", async () => {
      throw callError("unavailable");
    });
    fake.next("remove", async () => {
      throw callError("unavailable");
    });
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.zip" });
    await user.click(await fileMenu(user, "Clear finished…"));
    const dialog = screen.getByRole("dialog", { name: "Clear finished downloads?" });
    await user.click(within(dialog).getByRole("button", { name: "Clear finished" }));
    expect(await within(dialog).findByText("0 of 2 cleared; 2 kept.")).toBeVisible();
    const failures = within(dialog).getAllByRole("listitem");
    expect(failures).toHaveLength(2);
    for (const failure of failures) {
      expect(failure).toHaveTextContent(
        /The download service could not be reached\. Nothing changed\./,
      );
    }
    const close = within(dialog).getAllByRole("button", { name: "Close" })[0];
    if (!close) throw new Error("the failure dialog has no Close button");
    await user.click(close);
    expect(screen.getByRole("button", { name: "a.zip" })).toBeVisible();
    expect(screen.getByRole("button", { name: "b.zip" })).toBeVisible();
  });

  it("never lets an older list answer bring cleared rows back", async () => {
    const fake = new FakeServiceClient();
    const a = snapshot({ job_id: "job_a", display_name: "a.zip", phase: "completed" });
    const b = snapshot({ job_id: "job_b", display_name: "b.zip", phase: "completed" });
    fake.items = [a, b];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.zip" });
    const late = deferred<DownloadSnapshot[]>();
    const before = fake.count("list");
    fake.next("list", () => late.promise);
    await waitFor(() => expect(fake.count("list")).toBe(before + 1));
    await user.click(await fileMenu(user, "Clear finished…"));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Clear finished" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await act(async () => late.resolve([a, b]));
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(screen.queryByRole("button", { name: "a.zip" })).toBeNull();
    expect(screen.queryByRole("button", { name: "b.zip" })).toBeNull();
  });

  it("is unavailable when there is nothing finished to clear", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_a", display_name: "a.iso" })];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.iso" });
    const item = await fileMenu(user, "Clear finished…");
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription(/no completed, failed or cancelled/i);
  });
});
