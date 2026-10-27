// B2.2: close behaviour in Options, the one "Exit and stop downloads" confirmation (from the
// Tasks menu and the tray), its failures, and the plain-browser state.
import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { BrowserShell, type DesktopShell, type ExitReport } from "../services/service-client";
import { callError, deferred, FakeServiceClient, FakeShell } from "../test/fake-service";

function renderWith(shell: DesktopShell) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={new FakeServiceClient()} shell={shell} pollIntervalMs={50}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

async function openExitFromMenu(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("menuitem", { name: "Tasks" }));
  await user.click(screen.getByRole("menuitem", { name: "Exit and stop downloads" }));
  return screen.getByRole("dialog", { name: "Exit and stop downloads?" });
}

describe("close behaviour", () => {
  it("shows the desktop setting and persists a change through the shell", async () => {
    const shell = new FakeShell();
    const user = userEvent.setup();
    renderWith(shell);
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    let dialog = screen.getByRole("dialog", { name: "Options" });
    const select = await within(dialog).findByLabelText("When the window is closed");
    expect(select).toHaveValue("hide_to_tray");
    expect(select).toHaveAccessibleDescription(/keep running in the background service/);
    await user.selectOptions(select, "close_window");
    expect(shell.saves).toEqual(["close_window"]);
    await user.keyboard("{Escape}");
    // Reopening reads the saved value back from the shell.
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    dialog = screen.getByRole("dialog", { name: "Options" });
    expect(await within(dialog).findByLabelText("When the window is closed")).toHaveValue(
      "close_window",
    );
  });

  it("says when there is no tray and when saving failed", async () => {
    const shell = new FakeShell();
    shell.settings = { close_behaviour: "hide_to_tray", tray_available: false };
    shell.failSave = true;
    const user = userEvent.setup();
    renderWith(shell);
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    const dialog = screen.getByRole("dialog", { name: "Options" });
    const select = await within(dialog).findByLabelText("When the window is closed");
    expect(select).toHaveAccessibleDescription(/minimizes the window instead/);
    await user.selectOptions(select, "close_window");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("could not be saved");
    expect(select).toHaveValue("hide_to_tray");
  });

  it("keeps the plain browser honest: no close setting and no exit", async () => {
    const user = userEvent.setup();
    renderWith(new BrowserShell());
    await user.click(screen.getByRole("menuitem", { name: "Tasks" }));
    const exit = screen.getByRole("menuitem", { name: "Exit and stop downloads" });
    expect(exit).toHaveAttribute("aria-disabled", "true");
    expect(exit).toHaveAccessibleDescription(/only the Downova desktop app/i);
    await user.keyboard("{Escape}");
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    expect(screen.getByText("Only the Downova desktop app has this setting.")).toBeVisible();
  });
});

describe("Exit and stop downloads", () => {
  it("does nothing when cancelled", async () => {
    const shell = new FakeShell();
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openExitFromMenu(user);
    expect(within(dialog).getByText(/stay paused until you resume them/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(shell.exits).toBe(0);
  });

  it("sends one request however often it is confirmed, and reports the verified stop", async () => {
    const shell = new FakeShell();
    const reply = deferred<ExitReport>();
    shell.nextExit(() => reply.promise);
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openExitFromMenu(user);
    await user.click(within(dialog).getByRole("button", { name: "Exit and stop downloads" }));
    const stopping = within(dialog).getByRole("button", { name: "Stopping…" });
    expect(stopping).toHaveAttribute("aria-disabled", "true");
    await user.click(stopping);
    await user.click(stopping);
    expect(shell.exits).toBe(1);
    await act(async () => reply.resolve({ paused_jobs: 2, stopped: true }));
    expect(
      within(dialog).getByText("The download service has stopped. Downova is closing."),
    ).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Exit and stop downloads" })).toBeNull();
  });

  it("opens the same confirmation from the tray", async () => {
    const shell = new FakeShell();
    renderWith(shell);
    await waitFor(() => expect(shell.labels.length).toBeGreaterThan(0));
    act(() => shell.requestExit());
    expect(screen.getByRole("dialog", { name: "Exit and stop downloads?" })).toBeVisible();
    expect(shell.exits).toBe(0);
  });

  it("explains each kind of failure and lets the user try again", async () => {
    const shell = new FakeShell();
    shell
      .nextExit(async () => {
        throw callError("unavailable");
      })
      .nextExit(async () => {
        throw callError("uncertain");
      })
      .nextExit(async () => {
        throw callError("rejected", "INVALID_REQUEST", "none");
      })
      .nextExit(async () => ({ paused_jobs: 1, stopped: false }));
    const user = userEvent.setup();
    renderWith(shell);
    const dialog = await openExitFromMenu(user);
    const confirm = () =>
      user.click(within(dialog).getByRole("button", { name: "Exit and stop downloads" }));
    await confirm();
    expect(
      await within(dialog).findByText(/could not be reached. Nothing was stopped/),
    ).toBeVisible();
    await confirm();
    expect(await within(dialog).findByText(/did not confirm in time/)).toBeVisible();
    await confirm();
    expect(
      await within(dialog).findByText(
        "The request or the server's response could not be accepted.",
      ),
    ).toBeVisible();
    await confirm();
    expect(await within(dialog).findByText(/still running/)).toBeVisible();
    expect(shell.exits).toBe(4);
  });

  it("labels the tray in the window's language", async () => {
    const shell = new FakeShell();
    const user = userEvent.setup();
    renderWith(shell);
    await waitFor(() =>
      expect(shell.labels.at(-1)).toEqual(["Open Downova", "Exit and stop downloads"]),
    );
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    await user.selectOptions(screen.getByLabelText("Language"), "bn");
    await waitFor(() =>
      expect(shell.labels.at(-1)).toEqual(["Downova খুলুন", "বন্ধ করুন ও ডাউনলোড থামান"]),
    );
  });
});
