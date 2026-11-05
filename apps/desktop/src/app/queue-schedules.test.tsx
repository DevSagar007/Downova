import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell } from "../test/fake-service";
import { bn, en } from "../i18n/messages";
import { formatInstant, scheduleProblem } from "../features/queues/QueueScheduleEditor";

/** The value, or a test failure when it is missing. */
function must<T>(v: T | undefined | null): T {
  if (v === undefined || v === null) throw new Error("missing");
  return v;
}

function setup(fake = new FakeServiceClient()) {
  fake.streaming = true;
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { fake, user: userEvent.setup() };
}

async function manageQueues(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("menuitem", { name: "Downloads" }));
  await user.click(screen.getByRole("menuitem", { name: "Manage queues…" }));
  const dialog = screen.getByRole("dialog", { name: "Manage queues" });
  return within(must(within(dialog).getAllByRole("listitem")[0]));
}

describe("queue schedules (CORE-08)", () => {
  it("saves weekly windows in a zone with a missed-window policy, shows the next window, and removes it", async () => {
    const { fake, user } = setup();
    const main = await manageQueues(user);
    const group = main.getByRole("group", { name: "Schedule" });
    const g = within(group);
    const zone = g.getByRole("textbox", { name: "Time zone" });
    await user.clear(zone);
    await user.type(zone, "Asia/Dhaka");
    // Window 1: weekdays 01:00–07:00 (the defaults); add an overnight weekend window.
    await user.click(g.getByRole("button", { name: "Add window" }));
    const second = within(g.getByRole("group", { name: "Window 2" }));
    for (const d of ["Mon", "Tue", "Wed", "Thu", "Fri"]) {
      await user.click(second.getByRole("checkbox", { name: d }));
    }
    await user.click(second.getByRole("checkbox", { name: "Sat" }));
    await user.click(second.getByRole("checkbox", { name: "Sun" }));
    const from = second.getByLabelText("From");
    await user.clear(from);
    await user.type(from, "22:00");
    const until = second.getByLabelText("Until");
    await user.clear(until);
    await user.type(until, "08:00");
    await user.selectOptions(
      g.getByRole("combobox", { name: "If a window is missed" }),
      "catch_up",
    );
    await user.click(g.getByRole("button", { name: "Save schedule" }));
    await waitFor(() => expect(fake.count("queueSchedule")).toBe(1));
    const [queueId, json] = must(fake.calls.find((c) => c.method === "queueSchedule")).args;
    expect(queueId).toBe("main");
    expect(JSON.parse(must(json))).toEqual({
      enabled: true,
      timezone: "Asia/Dhaka",
      windows: [
        { days: ["mon", "tue", "wed", "thu", "fri"], start: "01:00", end: "07:00" },
        { days: ["sat", "sun"], start: "22:00", end: "08:00" },
      ],
      missed_window: "catch_up",
    });
    // The service's evaluation, in the schedule's zone (19:00Z Thu = 01:00 Fri in Dhaka).
    const state = await within(
      must((await screen.findAllByRole("group", { name: "Schedule" }))[0]),
    ).findByRole("status", { name: "Schedule" });
    expect(state).toHaveTextContent("Waiting for the next window: Fri 01:00.");
    expect(
      screen.getAllByText(/cannot download while the computer is off or asleep/)[0],
    ).toBeVisible();
    await user.click(must(screen.getAllByRole("button", { name: "Remove schedule" })[0]));
    await waitFor(() => expect(fake.count("queueSchedule")).toBe(2));
    expect(must(fake.calls.filter((c) => c.method === "queueSchedule")[1]).args).toEqual([
      "main",
      "null",
    ]);
    expect(must(fake.queues[0]).schedule).toBeUndefined();
  });

  it("refuses a window without days or with equal times before sending anything", async () => {
    const { fake, user } = setup();
    const main = await manageQueues(user);
    const g = within(main.getByRole("group", { name: "Schedule" }));
    const until = g.getByLabelText("Until");
    await user.clear(until);
    await user.type(until, "01:00");
    await user.click(g.getByRole("button", { name: "Save schedule" }));
    expect(g.getByRole("alert")).toHaveTextContent("they must differ");
    expect(fake.count("queueSchedule")).toBe(0);
  });

  it("checks schedules and formats instants in the schedule's zone", () => {
    const base = { enabled: true, timezone: "UTC", missed_window: "skip" as const };
    expect(
      scheduleProblem({ ...base, windows: [{ days: [] as never, start: "01:00", end: "02:00" }] }),
    ).toBe("days");
    expect(
      scheduleProblem({
        ...base,
        timezone: " ",
        windows: [{ days: ["mon"], start: "01:00", end: "02:00" }],
      }),
    ).toBe("zone");
    expect(
      scheduleProblem({ ...base, windows: [{ days: ["mon"], start: "22:00", end: "06:00" }] }),
    ).toBeNull();
    expect(formatInstant("2026-11-05T19:00:00Z", "Asia/Dhaka", "en")).toBe("Fri 01:00");
  });

  it("has every schedule string in English and Bangla", () => {
    const keys = Object.keys(en).filter((k) => k.startsWith("schedule."));
    expect(keys.length).toBeGreaterThan(20);
    for (const k of keys) {
      expect(bn[k as keyof typeof bn], k).toBeTruthy();
      expect(bn[k as keyof typeof bn], k).not.toBe(en[k as keyof typeof en]);
    }
  });
});
