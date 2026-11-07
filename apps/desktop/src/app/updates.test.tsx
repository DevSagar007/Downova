// ADV09 (ADR 0030): Help → Check for updates…: "not configured" is never "up to date"; the full
// check → verify → prepare → confirmed install path; one step at a time; stale events ignored;
// failures and recovery offer only safe actions; the automatic check is opt-in; Bangla.
import { afterEach, describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { BrowserShell, type DesktopShell, type UpdateView } from "../services/service-client";
import { AUTO_CHECK_KEY } from "../features/support/UpdateDialog";
import { deferred, FakeServiceClient, FakeShell } from "../test/fake-service";

function renderWith(shell: DesktopShell) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={new FakeServiceClient()} shell={shell} pollIntervalMs={50}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

async function openUpdates(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("menuitem", { name: "Help" }));
  await user.click(screen.getByRole("menuitem", { name: "Check for updates…" }));
  return screen.getByRole("dialog", { name: "Updates" });
}

afterEach(() => localStorage.removeItem(AUTO_CHECK_KEY));

describe("updates", () => {
  it("says 'not configured' (never 'up to date') and offers no update action", async () => {
    const shell = new FakeShell();
    shell.update = { ...shell.update, view: { state: "not_configured", reason: "missing" } };
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openUpdates(user);
    expect(await within(dialog).findByText("Updates are not configured")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/does not mean you have the newest version/),
    ).toBeInTheDocument();
    expect(within(dialog).queryByText(/No newer version/)).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Check for updates" })).toBeNull();
    expect(shell.updateCalls).toEqual([]);
  });

  it("a package-manager build and the plain browser say why they do not update", async () => {
    const shell = new FakeShell();
    shell.update = {
      ...shell.update,
      view: { state: "not_configured", reason: "package_manager" },
    };
    const user = userEvent.setup();
    const { unmount } = renderWith(shell);
    let dialog = await openUpdates(user);
    expect(await within(dialog).findByText(/system's package manager/)).toBeInTheDocument();
    unmount();
    renderWith(new BrowserShell());
    dialog = await openUpdates(user);
    expect(await within(dialog).findByText(/not an installed Downova package/)).toBeInTheDocument();
  });

  it("checks, verifies, prepares and installs only after an explicit confirmation", async () => {
    const shell = new FakeShell();
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openUpdates(user);
    await user.click(await within(dialog).findByRole("button", { name: "Check for updates" }));
    expect(await within(dialog).findByText("Downova 0.2.0 is available.")).toBeInTheDocument();
    expect(within(dialog).getByText(/Nothing is installed until you confirm/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Download and verify" }));
    expect(await within(dialog).findByText("Verified")).toBeInTheDocument();
    expect(within(dialog).getByText(/signed by the trusted update key/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Prepare to install" }));
    expect(await within(dialog).findByText("Ready to install")).toBeInTheDocument();
    expect(within(dialog).getByText(/2 running download\(s\) are held/)).toBeInTheDocument();
    // The first click only asks; nothing is installed yet.
    await user.click(within(dialog).getByRole("button", { name: "Install…" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Install Downova 0.2.0 now?");
    expect(shell.updateCalls.some((c) => c.startsWith("install"))).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Install now" }));
    expect(await within(dialog).findByText(/Installing/)).toBeInTheDocument();
    expect(shell.updateCalls).toEqual([
      "check",
      "download:upd_fake_0001",
      "prepare:upd_fake_0001",
      "install:upd_fake_0001",
    ]);
  });

  it("runs one step at a time and ignores events of an older operation", async () => {
    const shell = new FakeShell();
    shell.update = {
      ...shell.update,
      view: {
        state: "available",
        operation_id: "upd_op_new",
        version: "0.3.0",
        date: null,
        notes: null,
      },
    };
    const slow = deferred<UpdateView>();
    shell.nextUpdate("download:upd_op_new", () => slow.promise);
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openUpdates(user);
    const download = await within(dialog).findByRole("button", { name: "Download and verify" });
    await user.click(download);
    await user.click(download);
    expect(shell.updateCalls.filter((c) => c.startsWith("download"))).toHaveLength(1);
    // A late event from an operation that is no longer current changes nothing.
    act(() =>
      shell.pushUpdate({ state: "verified", operation_id: "upd_op_old", version: "0.2.0" }),
    );
    expect(within(dialog).queryByText("Verified")).toBeNull();
    await act(async () =>
      slow.resolve({ state: "verified", operation_id: "upd_op_new", version: "0.3.0" }),
    );
    expect(await within(dialog).findByText("Verified")).toBeInTheDocument();
  });

  it("a rejected update and a failed preparation say nothing was installed", async () => {
    const shell = new FakeShell();
    shell.update = {
      ...shell.update,
      view: {
        state: "failed",
        operation_id: "upd_op_x",
        stage: "verify",
        error: { kind: "CHECKSUM_MISMATCH", retryable: false, next_action: "none", detail: null },
        can_retry_prepare: false,
      },
    };
    const user = userEvent.setup();
    const { unmount } = renderWith(shell);
    let dialog = await openUpdates(user);
    expect(await within(dialog).findByText("The update was rejected")).toBeInTheDocument();
    expect(within(dialog).getByText(/Nothing was installed/)).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Prepare again" })).toBeNull();
    unmount();
    shell.update = {
      ...shell.update,
      view: {
        state: "failed",
        operation_id: "upd_op_y",
        stage: "prepare",
        error: { kind: "INTERNAL", retryable: false, next_action: "retry", detail: null },
        can_retry_prepare: true,
      },
    };
    renderWith(shell);
    dialog = await openUpdates(user);
    await user.click(await within(dialog).findByRole("button", { name: "Prepare again" }));
    await waitFor(() => expect(shell.updateCalls).toContain("prepare:upd_op_y"));
  });

  it("recovery offers try again or a confirmed restore of the checked backup", async () => {
    const shell = new FakeShell();
    shell.update = {
      ...shell.update,
      view: { state: "recovery_required", kind: "migration_failed", backup_operation: "upd_op_b" },
    };
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openUpdates(user);
    expect(await within(dialog).findByText("Recovery required")).toBeInTheDocument();
    expect(within(dialog).getByText(/could not upgrade your download list/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Restore the backup…" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("kept aside, not deleted");
    expect(shell.updateCalls).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Restore now" }));
    await waitFor(() => expect(shell.updateCalls).toEqual(["restore:upd_op_b"]));
  });

  it("checks automatically only after the user opted in, and only checks", async () => {
    const shell = new FakeShell();
    const user = userEvent.setup();
    const { unmount } = renderWith(shell);
    await screen.findByRole("menuitem", { name: "Help" });
    expect(shell.updateCalls).toEqual([]);
    const dialog = await openUpdates(user);
    await user.click(await within(dialog).findByLabelText(/automatically when Downova starts/));
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("1");
    unmount();
    const next = new FakeShell();
    renderWith(next);
    await waitFor(() => expect(next.updateCalls).toEqual(["check"]));
  });

  it("speaks Bangla", async () => {
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ locale: "bn" }));
    const shell = new FakeShell();
    shell.update = { ...shell.update, view: { state: "not_configured", reason: "missing" } };
    const user = userEvent.setup();
    renderWith(shell);
    await user.click(screen.getByRole("menuitem", { name: "সাহায্য" }));
    await user.click(screen.getByRole("menuitem", { name: "আপডেট খুঁজুন…" }));
    const dialog = screen.getByRole("dialog", { name: "আপডেট" });
    expect(await within(dialog).findByText("আপডেট চালু করা নেই")).toBeInTheDocument();
    localStorage.removeItem("dm.ui.preferences.v1");
  });
});
