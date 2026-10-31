// B2.12 (CORE-10): Options → History. Retention is the service's; the window shows the exact
// count before anything leaves the list and says that files stay on disk.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";

function setup() {
  const fake = new FakeServiceClient();
  fake.streaming = true;
  fake.items = [
    snapshot({ job_id: "job_old1", display_name: "old-1.zip", phase: "completed" }),
    snapshot({ job_id: "job_old2", display_name: "old-2.iso", phase: "failed" }),
    snapshot({ job_id: "job_new", display_name: "new.pdf", phase: "completed" }),
    snapshot({ job_id: "job_run", display_name: "running.bin" }),
  ];
  fake.expired = new Set(["job_old1", "job_old2"]);
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()} reconnectDelaysMs={[20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { fake, user: userEvent.setup() };
}

async function openHistory(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByRole("button", { name: "old-1.zip" });
  await user.click(screen.getByRole("button", { name: "Options" }));
  const dialog = screen.getByRole("dialog", { name: "Options" });
  return {
    dialog,
    select: await within(dialog).findByLabelText("Keep finished downloads in the list"),
  };
}

describe("history retention (CORE-10)", () => {
  it("keeps history forever by default and says files stay on disk", async () => {
    const { user } = setup();
    const { dialog, select } = await openHistory(user);
    expect(select).toHaveValue("");
    expect(select).toHaveAccessibleDescription(/files always stay on disk/i);
    expect(within(dialog).getByRole("button", { name: "Clean now" })).toHaveAccessibleDescription(
      /kept forever/,
    );
    expect(within(dialog).getByRole("button", { name: "Apply" })).toHaveAccessibleDescription(
      /already the saved setting/,
    );
  });

  it("asks with the exact count before removing, and Keep them changes nothing", async () => {
    const { fake, user } = setup();
    const { dialog, select } = await openHistory(user);
    await user.selectOptions(select, "30");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent(/2 of 3 finished downloads/);
    expect(alert).toHaveTextContent(/files stay on disk/i);
    await user.click(within(alert).getByRole("button", { name: "Keep them" }));
    expect(fake.count("historySet")).toBe(0);
    expect(fake.count("historyClean")).toBe(0);
    expect(fake.retentionDays).toBeNull();
    expect(screen.getByRole("button", { name: "old-1.zip" })).toBeVisible();
  });

  it("applies and removes only the confirmed entries; running and newer jobs stay", async () => {
    const { fake, user } = setup();
    const { dialog, select } = await openHistory(user);
    await user.selectOptions(select, "30");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    const alert = await within(dialog).findByRole("alert");
    await user.click(within(alert).getByRole("button", { name: "Apply and remove" }));
    expect(
      await within(dialog).findByText(/2 finished downloads were removed from the list/),
    ).toBeVisible();
    expect(fake.retentionDays).toBe(30);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "old-1.zip" })).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("button", { name: "old-2.iso" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "new.pdf" })).toBeVisible();
    expect(screen.getByRole("button", { name: "running.bin" })).toBeVisible();
  });

  it("saves without asking when nothing would leave the list now", async () => {
    const { fake, user } = setup();
    fake.expired = new Set();
    const { dialog, select } = await openHistory(user);
    await user.selectOptions(select, "365");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(await within(dialog).findByText("History setting saved.")).toBeVisible();
    expect(fake.retentionDays).toBe(365);
    expect(fake.count("historyClean")).toBe(0);
  });

  it("Clean now applies the saved policy after the same confirmation", async () => {
    const { fake, user } = setup();
    fake.retentionDays = 7;
    const { dialog } = await openHistory(user);
    await user.click(within(dialog).getByRole("button", { name: "Clean now" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent(/2 of 3/);
    await user.click(within(alert).getByRole("button", { name: "Remove from list" }));
    expect(await within(dialog).findByText(/2 finished downloads were removed/)).toBeVisible();
    expect(fake.count("historySet")).toBe(0);
  });

  it("reports a refused change honestly", async () => {
    const { fake, user } = setup();
    fake.expired = new Set();
    fake.next("historySet", async () => {
      throw callError("rejected", "INTERNAL", "retry");
    });
    const { dialog, select } = await openHistory(user);
    await user.selectOptions(select, "90");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(await within(dialog).findByText("The history setting was not changed.")).toBeVisible();
    expect(fake.retentionDays).toBeNull();
  });
});
