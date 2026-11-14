import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";
import { pendingWork } from "../features/downloads/finish";

function renderApp(client: FakeServiceClient, shell: FakeShell) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={client} shell={shell}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

async function openFinish(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("menuitem", { name: "Downloads" }));
  await user.click(screen.getByRole("menuitem", { name: "When downloads finish…" }));
  return screen.findByRole("dialog", { name: "When downloads finish" });
}

describe("F08 pendingWork", () => {
  it("waits for every wanted download until it has ended, not for manual pauses", () => {
    expect(
      pendingWork([
        snapshot({ job_id: "job_a", phase: "transferring" }),
        snapshot({ job_id: "job_b", phase: "verifying" }),
        snapshot({ job_id: "job_c", phase: "scanning" }),
        snapshot({ job_id: "job_d", phase: "queued", desired_state: "paused" }),
        snapshot({ job_id: "job_e", phase: "completed" }),
        snapshot({ job_id: "job_f", phase: "failed" }),
      ]),
    ).toBe(3);
  });
});

describe("F08 When downloads finish", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("is unavailable with nothing running and shows what the system cannot do", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    shell.power = { sleep: false, shutdown: true };
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_done", phase: "completed" })];
    renderApp(fake, shell);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    const dialog = await openFinish(user);
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(await within(dialog).findByRole("radio", { name: /Sleep/ })).toBeDisabled();
    expect(within(dialog).getByText(/not available on this system/)).toBeVisible();
  });

  it("counts down once downloads end, can be cancelled, and otherwise runs once", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
    const shell = new FakeShell();
    const fake = new FakeServiceClient();
    fake.streaming = true;
    fake.items = [snapshot({ job_id: "job_r", display_name: "r.bin", phase: "transferring" })];
    renderApp(fake, shell);
    expect(await screen.findByText("r.bin")).toBeVisible();

    // Armed, then cancelled during the countdown: nothing runs and the choice is forgotten.
    let dialog = await openFinish(user);
    await user.click(await within(dialog).findByRole("radio", { name: "Shut down" }));
    await user.click(within(dialog).getByRole("button", { name: "Turn on" }));
    expect(screen.getByText("The computer will shut down when downloads finish.")).toBeVisible();
    act(() =>
      fake.change(snapshot({ job_id: "job_r", display_name: "r.bin", phase: "completed" })),
    );
    const countdown = await screen.findByRole("dialog", { name: "Shutting down soon" });
    expect(within(countdown).getByRole("timer")).toHaveTextContent("60 seconds");
    expect(shell.shown).toBe(1);
    // The footer's Cancel (the dialog's close button carries the same name).
    const footerCancel = within(countdown).getAllByRole("button", { name: "Cancel" }).at(-1);
    if (!footerCancel) throw new Error("no Cancel button");
    await user.click(footerCancel);
    expect(screen.queryByText("The computer will shut down when downloads finish.")).toBeNull();
    expect(shell.powerRuns).toEqual([]);

    // Armed again with work running: when it ends and the countdown runs out, sleep runs once.
    act(() =>
      fake.change(
        snapshot({ job_id: "job_s", display_name: "s.bin", phase: "transferring" }),
        "job_added",
      ),
    );
    expect(await screen.findByText("s.bin")).toBeVisible();
    dialog = await openFinish(user);
    await user.click(await within(dialog).findByRole("radio", { name: "Sleep" }));
    await user.click(within(dialog).getByRole("button", { name: "Turn on" }));
    // Flushed under the fake clock (a findBy poll would run the countdown on the real timers).
    await act(async () => {
      fake.change(snapshot({ job_id: "job_s", display_name: "s.bin", phase: "completed" }));
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.getByRole("dialog", { name: "Sleeping soon" })).toBeVisible();
    for (let second = 0; second < 59; second += 1) {
      act(() => {
        vi.advanceTimersByTime(1_000);
      });
    }
    expect(screen.getByRole("timer")).toHaveTextContent("In 1 second");
    expect(shell.powerRuns).toEqual([]);
    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    await waitFor(() => expect(shell.powerRuns).toEqual(["sleep"]));
    expect(screen.queryByRole("dialog", { name: "Sleeping soon" })).toBeNull();
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(shell.powerRuns).toEqual(["sleep"]);
  });
});
