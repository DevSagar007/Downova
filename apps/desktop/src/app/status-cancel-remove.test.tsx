// B2.3: status icons and progress, confirmed Cancel download, durable Remove from list.
import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, deferred, FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";
import { progressPermille, statusKind } from "../features/downloads/present";

function renderLive(fake: FakeServiceClient, pollIntervalMs = 20) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()} pollIntervalMs={pollIntervalMs}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

const row = (name: RegExp) => screen.getByRole("row", { name });

async function fileMenu(user: ReturnType<typeof userEvent.setup>, item: string) {
  await user.click(screen.getByRole("menuitem", { name: "File" }));
  return screen.getByRole("menuitem", { name: item });
}

describe("status presentation", () => {
  it("computes progress exactly for huge counts and never for unknown or zero totals", () => {
    const big = snapshot({
      downloaded_bytes: "9007199254740993",
      total_bytes: "18014398509481986",
    });
    expect(progressPermille(big)).toBe(500);
    expect(progressPermille(snapshot({ downloaded_bytes: "5", total_bytes: null }))).toBeNull();
    expect(progressPermille(snapshot({ downloaded_bytes: "0", total_bytes: "0" }))).toBeNull();
    expect(progressPermille(snapshot({ downloaded_bytes: "12", total_bytes: "10" }))).toBe(1000);
    expect(
      progressPermille(snapshot({ phase: "completed", downloaded_bytes: "1", total_bytes: "1" })),
    ).toBeNull();
    // Manual pause outranks policy blockers; all bytes received is still not "completed".
    expect(statusKind(snapshot({ desired_state: "paused", blockers: ["offline"] }))).toBe("paused");
    expect(statusKind(snapshot({ blockers: ["disk_unavailable"] }))).toBe("waiting");
    expect(
      statusKind(snapshot({ phase: "verifying", downloaded_bytes: "4", total_bytes: "4" })),
    ).toBe("verifying");
  });

  it("shows a labelled progress bar only while unfinished with a known total", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_a", display_name: "a.iso", downloaded_bytes: "1", total_bytes: "4" }),
      snapshot({
        job_id: "job_b",
        display_name: "b.bin",
        downloaded_bytes: "7",
        total_bytes: null,
      }),
      snapshot({
        job_id: "job_c",
        display_name: "c.zip",
        phase: "completed",
        downloaded_bytes: "4",
        total_bytes: "4",
      }),
      snapshot({
        job_id: "job_d",
        display_name: "d.pdf",
        phase: "finalizing",
        downloaded_bytes: "4",
        total_bytes: "4",
      }),
    ];
    renderLive(fake);
    const bar = await screen.findByRole("progressbar", { name: "Progress of a.iso" });
    expect(bar).toHaveAttribute("value", "250");
    expect(bar).toHaveAttribute("aria-valuetext", "25.0%");
    expect(within(row(/b\.bin/)).queryByRole("progressbar")).toBeNull();
    expect(within(row(/c\.zip/)).queryByRole("progressbar")).toBeNull();
    // Every byte received, yet the status says Finishing, not Complete.
    expect(within(row(/d\.pdf/)).getByText("Finishing")).toBeVisible();
    expect(within(row(/d\.pdf/)).getByRole("progressbar")).toHaveAttribute("value", "1000");
  });
});

describe("Cancel download", () => {
  it("sends nothing when kept, and once when confirmed repeatedly", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_c1", display_name: "c1.bin" })];
    const ack = deferred<DownloadSnapshot>();
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "c1.bin" }));
    await user.click(await fileMenu(user, "Cancel download…"));
    let dialog = screen.getByRole("dialog", { name: "Cancel this download?" });
    expect(within(dialog).getByText(/Nothing is deleted/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Keep downloading" }));
    expect(fake.count("cancel")).toBe(0);

    fake.next("cancel", () => ack.promise);
    await user.click(await fileMenu(user, "Cancel download…"));
    dialog = screen.getByRole("dialog", { name: "Cancel this download?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel download" }));
    const pending = within(dialog).getByRole("button", { name: "Cancelling…" });
    await user.click(pending);
    expect(fake.count("cancel")).toBe(1);
    // Still not shown as cancelled until the service acknowledges.
    expect(within(row(/c1\.bin/)).queryByText("Cancelled")).toBeNull();
    const cancelled = snapshot({
      job_id: "job_c1",
      display_name: "c1.bin",
      desired_state: "cancelled",
      phase: "cancelled",
    });
    fake.items = [cancelled];
    await act(async () => ack.resolve(cancelled));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(within(row(/c1\.bin/)).getByText("Cancelled")).toBeVisible();
  });

  it("explains a refusal and keeps the job as it was", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_c2", display_name: "c2.bin", phase: "verifying" })];
    fake.next("cancel", async () => {
      throw callError("rejected", "INVALID_REQUEST", "none");
    });
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "c2.bin" }));
    await user.click(await fileMenu(user, "Cancel download…"));
    const dialog = screen.getByRole("dialog", { name: "Cancel this download?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel download" }));
    expect(await within(dialog).findByText("The download was not cancelled")).toBeVisible();
    expect(
      within(dialog).getByText("The request or the server's response could not be accepted."),
    ).toBeVisible();
    expect(within(row(/c2\.bin/)).getByText("Verifying")).toBeVisible();
  });

  it("is not offered once the file is being placed, or for finished downloads", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_f1", display_name: "f1.bin", phase: "finalizing" }),
      snapshot({ job_id: "job_f2", display_name: "f2.bin", phase: "completed" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "f1.bin" }));
    let item = await fileMenu(user, "Cancel download…");
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription(/can no longer be cancelled/);
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "f2.bin" }));
    item = await fileMenu(user, "Cancel download…");
    expect(item).toHaveAccessibleDescription(/already complete/);
  });

  it("keeps a cancelled row cancelled when an older list answer arrives late", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_c3", display_name: "c3.bin" })];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "c3.bin" }));
    const late = deferred<DownloadSnapshot[]>();
    const before = fake.count("list");
    fake.next("list", () => late.promise);
    await waitFor(() => expect(fake.count("list")).toBe(before + 1));
    await user.click(await fileMenu(user, "Cancel download…"));
    const dialog = screen.getByRole("dialog", { name: "Cancel this download?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel download" }));
    expect(await within(row(/c3\.bin/)).findByText("Cancelled")).toBeVisible();
    await act(async () =>
      late.resolve([
        snapshot({ job_id: "job_c3", display_name: "c3.bin", downloaded_bytes: "99" }),
      ]),
    );
    expect(within(row(/c3\.bin/)).getByText("Cancelled")).toBeVisible();
  });
});

describe("Remove from list", () => {
  it("is offered only for finished downloads and is distinct from Delete", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_r1", display_name: "run.bin" }),
      snapshot({ job_id: "job_r2", display_name: "held.bin", desired_state: "paused" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    for (const name of ["run.bin", "held.bin"]) {
      await user.click(await screen.findByRole("button", { name }));
      const item = await fileMenu(user, "Remove from list…");
      expect(item).toHaveAttribute("aria-disabled", "true");
      expect(item).toHaveAccessibleDescription(/Cancel it first/);
      await user.keyboard("{Escape}");
    }
    const del = within(screen.getByRole("toolbar")).getByRole("button", { name: "Delete" });
    expect(del).toHaveAttribute("aria-disabled", "true");
    expect(del).toHaveAccessibleDescription(/keeps the file/);
    expect(fake.count("remove")).toBe(0);
  });

  it("removes a finished row, moves selection and focus to the next one, and announces it", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_d1", display_name: "one.zip", phase: "completed" }),
      snapshot({ job_id: "job_d2", display_name: "two.zip", phase: "failed" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "one.zip" }));
    await user.click(await fileMenu(user, "Remove from list…"));
    const dialog = screen.getByRole("dialog", { name: "Remove this download from the list?" });
    expect(within(dialog).getByText(/nothing is deleted/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Remove from list" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "one.zip" })).toBeNull());
    await waitFor(() => expect(screen.getByRole("button", { name: "two.zip" })).toHaveFocus());
    expect(screen.getByRole("button", { name: "two.zip" })).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByText("one.zip removed from the list. The file was kept."),
    ).toBeInTheDocument();
  });

  it("never lets an older list answer bring a removed row back", async () => {
    const fake = new FakeServiceClient();
    const done = snapshot({ job_id: "job_s1", display_name: "s1.zip", phase: "completed" });
    fake.items = [done];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "s1.zip" }));
    const late = deferred<DownloadSnapshot[]>();
    const before = fake.count("list");
    fake.next("list", () => late.promise);
    await waitFor(() => expect(fake.count("list")).toBe(before + 1));
    await user.click(await fileMenu(user, "Remove from list…"));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Remove from list" }),
    );
    await waitFor(() => expect(screen.queryByRole("button", { name: "s1.zip" })).toBeNull());
    await act(async () => late.resolve([done]));
    await new Promise((r) => setTimeout(r, 80));
    expect(screen.queryByRole("button", { name: "s1.zip" })).toBeNull();
  });

  it("keeps the row when the removal could not be recorded", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_e1", display_name: "e1.zip", phase: "cancelled" })];
    fake.next("remove", async () => {
      throw callError("rejected", "INTERNAL", "retry");
    });
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "e1.zip" }));
    await user.click(await fileMenu(user, "Remove from list…"));
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Remove from list" }));
    expect(await within(dialog).findByText("The download was not removed")).toBeVisible();
    expect(within(dialog).getByText("Try again in a moment.")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Keep in list" }));
    expect(screen.getByRole("button", { name: "e1.zip" })).toBeVisible();
  });

  it("works from the keyboard and in Bangla", async () => {
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ locale: "bn" }));
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_k1", display_name: "বই.pdf", phase: "completed" })];
    const user = userEvent.setup();
    renderLive(fake);
    const pick = await screen.findByRole("button", { name: "বই.pdf" });
    pick.focus();
    await user.keyboard("{Enter}");
    expect(pick).toHaveAttribute("aria-pressed", "true");
    const file = screen.getByRole("menuitem", { name: "ফাইল" });
    file.focus();
    await user.keyboard("{ArrowDown}");
    const item = screen.getByRole("menuitem", { name: "তালিকা থেকে সরান…" });
    await user.click(item);
    const dialog = screen.getByRole("dialog", { name: "এই ডাউনলোডটি তালিকা থেকে সরাবেন?" });
    await user.click(within(dialog).getByRole("button", { name: "তালিকা থেকে সরান" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "বই.pdf" })).toBeNull());
    expect(fake.calls.filter((c) => c.method === "remove").map((c) => c.args)).toEqual([
      ["job_k1"],
    ]);
  });
});
