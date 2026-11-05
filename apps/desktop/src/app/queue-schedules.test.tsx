import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";
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
    // Bangla: Bangla digits on a 24-hour clock, never a Latin AM/PM.
    expect(formatInstant("2026-11-05T08:39:00Z", "Asia/Dhaka", "bn")).toMatch(/১৪:৩৯$/);
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

/** A Night queue scheduled in Dhaka whose next window opens at `next` (UTC). */
function nightQueue(fake: FakeServiceClient, next: string | null) {
  fake.queues = [
    ...fake.queues.filter((q) => q.queue_id === "main"),
    {
      queue_id: "q_night",
      name: "Night",
      position: 1,
      max_active: 3,
      running: true,
      schedule: {
        enabled: true,
        timezone: "Asia/Dhaka",
        windows: [{ days: ["fri"], start: "01:00", end: "07:00" }],
        missed_window: "skip",
      },
      schedule_state: { in_window: false, next_change_at: next, catch_up_until: null },
    },
  ];
}

describe("a download's schedule state (CORE-08)", () => {
  it("names the schedule, not the slot limit, and shows the next window in the queue's zone", async () => {
    const fake = new FakeServiceClient();
    nightQueue(fake, "2099-11-05T19:00:00Z");
    fake.items = [
      snapshot({
        job_id: "job_held",
        display_name: "held.bin",
        phase: "queued",
        queue_id: "q_night",
        // Insertion order puts the slot limit first; the schedule is still the primary reason.
        blockers: ["queue_limit", "schedule_window"],
      }),
    ];
    const { user } = setup(fake);
    const row = await screen.findByRole("row", { name: /held\.bin/ });
    expect(row).toHaveTextContent("Waiting: queue schedule");
    expect(row).not.toHaveTextContent("all download slots in use");
    await user.click(within(row).getByRole("button", { name: "held.bin" }));
    expect(await screen.findByText("Waiting for the schedule of Night")).toBeVisible();
    expect(screen.getByText("Next window: Fri 01:00 (Asia/Dhaka).")).toBeVisible();
    // Keyboard: the note is readable text in the details, with no extra focus stop.
    expect(
      screen.getByText("Waiting for the schedule of Night").closest("[role=status]"),
    ).not.toBeNull();
  });

  it("explains a transfer finishing outside its window, and not once it is paused", async () => {
    const fake = new FakeServiceClient();
    nightQueue(fake, "2099-11-05T19:00:00Z");
    fake.items = [
      snapshot({
        job_id: "job_stream",
        display_name: "stream.ts",
        queue_id: "q_night",
        schedule_exemption: "no_resumable_checkpoint",
      }),
    ];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "stream.ts" }));
    expect(await screen.findByText("Finishing outside the schedule window")).toBeVisible();
    expect(screen.getByText(/would lose its progress/)).toBeVisible();
    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Stop" }));
    await waitFor(() =>
      expect(screen.queryByText("Finishing outside the schedule window")).not.toBeInTheDocument(),
    );
  });

  it("re-reads the queues once a shown window has passed", async () => {
    const fake = new FakeServiceClient();
    nightQueue(fake, new Date(Date.now() - 5000).toISOString());
    fake.items = [
      snapshot({
        job_id: "job_late",
        display_name: "late.bin",
        phase: "queued",
        queue_id: "q_night",
        blockers: ["schedule_window"],
      }),
    ];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "late.bin" }));
    expect(await screen.findByText("The window has changed; updating…")).toBeVisible();
    const before = fake.count("queues");
    nightQueue(fake, "2099-11-05T19:00:00Z");
    await waitFor(() => expect(fake.count("queues")).toBeGreaterThan(before), { timeout: 4000 });
    expect(await screen.findByText("Next window: Fri 01:00 (Asia/Dhaka).")).toBeVisible();
  });

  it("says it in Bangla", async () => {
    const fake = new FakeServiceClient();
    nightQueue(fake, "2099-11-05T19:00:00Z");
    fake.items = [
      snapshot({
        job_id: "job_bn",
        display_name: "bn.bin",
        phase: "queued",
        queue_id: "q_night",
        blockers: ["schedule_window"],
      }),
    ];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "bn.bin" }));
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    await user.click(await screen.findByRole("menuitemradio", { name: /বাংলা/ }));
    expect(await screen.findByText("Night-এর সময়সূচির অপেক্ষায়")).toBeVisible();
    expect(screen.getByText(/পরের সময়সীমা: .+ \(Asia\/Dhaka\)।/)).toBeVisible();
    expect(screen.getByRole("row", { name: /bn\.bin/ })).toHaveTextContent(
      "অপেক্ষা: কিউয়ের সময়সূচি",
    );
  });
});

describe("one save per click burst (CORE-08)", () => {
  it("two clicks on Save schedule in the same frame send one request", async () => {
    const { fake, user } = setup();
    const main = await manageQueues(user);
    const save = within(main.getByRole("group", { name: "Schedule" })).getByRole("button", {
      name: "Save schedule",
    });
    act(() => {
      save.click();
      save.click();
    });
    await waitFor(() => expect(fake.count("queueSchedule")).toBe(1));
    await act(async () => {});
    expect(fake.count("queueSchedule")).toBe(1);
  });
});
