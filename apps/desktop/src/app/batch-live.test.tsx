// B2.8–B2.11 in the window: the event stream (ARCH-08), attach-only reconnection (ARCH-09),
// duplicates and ask-on-collision (CORE-07), queues and priority (CORE-05). Every check goes
// through the ServiceClient boundary with the scriptable fake; nothing here invents service state.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, FakeServiceClient, FakeShell, HELLO, snapshot } from "../test/fake-service";

function renderLive(fake: FakeServiceClient, shell = new FakeShell()) {
  return render(
    <PreferencesProvider>
      <ServiceProvider
        client={fake}
        shell={shell}
        pollIntervalMs={20}
        reconnectDelaysMs={[20, 20, 20]}
      >
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

function streaming(items = [snapshot({ job_id: "job_a", display_name: "a.bin" })]) {
  const fake = new FakeServiceClient();
  fake.streaming = true;
  fake.items = items;
  return fake;
}

const toolbar = () => screen.getByRole("toolbar");
const command = (name: string) => within(toolbar()).getByRole("button", { name });
const row = (name: RegExp) => screen.getByRole("row", { name });

async function menu(user: ReturnType<typeof userEvent.setup>, top: string, item: string) {
  await user.click(screen.getByRole("menuitem", { name: top }));
  await user.click(screen.getByRole("menuitem", { name: item }));
}

describe("event stream (ARCH-08)", () => {
  it("starts from the subscription snapshot and applies ordered events without polling", async () => {
    const fake = streaming();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    act(() => {
      fake.change(snapshot({ job_id: "job_a", display_name: "a.bin", phase: "completed" }));
      fake.change(snapshot({ job_id: "job_b", display_name: "b.bin" }), "job_added");
    });
    expect(await within(row(/a\.bin/)).findByText("Complete")).toBeVisible();
    expect(await screen.findByRole("button", { name: "b.bin" })).toBeVisible();
    await new Promise((r) => setTimeout(r, 80));
    expect(fake.count("list")).toBe(0);
    expect(fake.subscriptions).toBe(1);
  });

  it("takes a fresh snapshot after a sequence gap, a new service instance or snapshot_required", async () => {
    const fake = streaming();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    // A gap: the event is not applied; a new subscription brings the authoritative list.
    fake.items = [snapshot({ job_id: "job_a", display_name: "a.bin", phase: "completed" })];
    act(() => {
      fake.emit(
        "job_changed",
        snapshot({ job_id: "job_a", display_name: "gap.bin" }),
        fake.sequence + 2n,
      );
    });
    await waitFor(() => expect(fake.subscriptions).toBe(2));
    expect(await within(row(/a\.bin/)).findByText("Complete")).toBeVisible();
    expect(screen.queryByRole("button", { name: "gap.bin" })).not.toBeInTheDocument();
    // Another service instance.
    act(() => {
      fake.instance = "svc_other";
      fake.emit("progress", snapshot({ job_id: "job_a", display_name: "a.bin" }));
    });
    await waitFor(() => expect(fake.subscriptions).toBe(3));
    // The service asks for a snapshot.
    act(() => {
      fake.emit("snapshot_required", null);
    });
    await waitFor(() => expect(fake.subscriptions).toBe(4));
    // Superseded subscriptions were closed.
    expect(fake.closedSubscriptions).toBeGreaterThanOrEqual(3);
  });

  it("keeps sequences exact beyond JavaScript's safe integers", async () => {
    const fake = streaming();
    fake.sequence = 9007199254740993n; // 2^53 + 1
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    act(() => {
      fake.change(snapshot({ job_id: "job_a", display_name: "a.bin", phase: "completed" }));
    });
    expect(await within(row(/a\.bin/)).findByText("Complete")).toBeVisible();
    expect(fake.sequence).toBe(9007199254740994n);
    expect(fake.subscriptions).toBe(1);
  });

  it("never brings a removed job back from a late update", async () => {
    const fake = streaming([
      snapshot({ job_id: "job_r", display_name: "r.bin", phase: "completed" }),
    ]);
    renderLive(fake);
    await screen.findByRole("button", { name: "r.bin" });
    act(() => {
      fake.items = [];
      fake.emit("job_removed", null);
    });
    act(() => {
      fake.emit("job_removed", { ...snapshot({ job_id: "job_r" }), removed: true });
      fake.emit("job_changed", snapshot({ job_id: "job_r", display_name: "r.bin" }));
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "r.bin" })).not.toBeInTheDocument(),
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByRole("button", { name: "r.bin" })).not.toBeInTheDocument();
  });

  it("polls only as the explicit fallback for a service without events", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_a", display_name: "a.bin" })];
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    await waitFor(() => expect(fake.count("list")).toBeGreaterThan(1));
    expect(fake.count("subscribe")).toBe(1);
  });
});

describe("lifecycle (ARCH-09)", () => {
  it("reattaches to a restarted service with a fresh snapshot and never starts one itself", async () => {
    const fake = streaming();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    const connects = fake.count("connect");
    // The service stops: the stream is lost and nothing answers.
    act(() => {
      fake.connection = { status: "unavailable", reason: "not_running", error: null };
      fake.endStream("lost");
    });
    expect(await screen.findByText(/Trying to reconnect/)).toBeVisible();
    // A new instance comes back with a changed list.
    fake.instance = "svc_restarted";
    fake.sequence = 0n;
    fake.items = [snapshot({ job_id: "job_a", display_name: "a.bin", desired_state: "paused" })];
    fake.connection = { status: "connected", hello: HELLO };
    expect(await within(row(/a\.bin/)).findByText("Paused")).toBeVisible();
    expect(fake.count("connect")).toBe(connects);
    expect(fake.count("probe")).toBeGreaterThan(0);
    // No mutation was replayed.
    expect(
      fake.calls.filter((c) => ["add", "pause", "resume", "cancel"].includes(c.method)),
    ).toEqual([]);
  });

  it("stops after the bounded attempts and offers Reconnect", async () => {
    const fake = streaming();
    renderLive(fake);
    await screen.findByRole("button", { name: "a.bin" });
    act(() => {
      fake.connection = { status: "unavailable", reason: "not_running", error: null };
      fake.endStream("lost");
    });
    expect(await screen.findByRole("button", { name: "Reconnect" })).toBeVisible();
    expect(fake.count("probe")).toBe(3);
  });

  it("does not reconnect after Exit and stop downloads", async () => {
    const fake = streaming();
    const shell = new FakeShell();
    const user = userEvent.setup();
    renderLive(fake, shell);
    await screen.findByRole("button", { name: "a.bin" });
    shell.nextExit(async () => {
      fake.connection = { status: "unavailable", reason: "not_running", error: null };
      fake.endStream("closed");
      return { paused_jobs: 1, stopped: true };
    });
    await menu(user, "Tasks", "Exit and stop downloads");
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Exit and stop downloads" }),
    );
    await screen.findByText(/stopped/i);
    const probes = fake.count("probe");
    const connects = fake.count("connect");
    await new Promise((r) => setTimeout(r, 120));
    expect(fake.count("connect")).toBe(connects);
    expect(fake.count("probe")).toBeLessThanOrEqual(probes + 1);
    expect(fake.subscriptions).toBe(1);
    expect(screen.queryByText(/Trying to reconnect/)).not.toBeInTheDocument();
  });
});

describe("duplicates and names (CORE-07)", () => {
  it("offers the existing download before creating another", async () => {
    const fake = streaming([snapshot({ job_id: "job_e", display_name: "e.iso" })]);
    const url = "https://h.example/e.iso?sig=A";
    fake.duplicates.set(url, [
      { job_id: "job_e", display_name: "e.iso", phase: "transferring", desired_state: "running" },
    ]);
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "e.iso" });
    await user.click(command("Add URL"));
    await user.type(screen.getByLabelText("Address (URL)"), url);
    await user.click(screen.getByRole("button", { name: "Add" }));
    const prompt = await screen.findByText("This link is already in the list");
    expect(prompt).toBeVisible();
    expect(fake.count("add")).toBe(0);
    await user.click(screen.getByRole("button", { name: "Show existing download" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "e.iso" })).toHaveAttribute("aria-pressed", "true");
    expect(fake.count("add")).toBe(0);
  });

  it("creates another download only on request, as a new request", async () => {
    const fake = streaming([snapshot({ job_id: "job_e", display_name: "e.iso" })]);
    const url = "https://h.example/e.iso";
    fake.duplicates.set(url, [
      { job_id: "job_e", display_name: "e.iso", phase: "transferring", desired_state: "running" },
    ]);
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "e.iso" });
    await user.click(command("Add URL"));
    await user.type(screen.getByLabelText("Address (URL)"), url);
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.click(await screen.findByRole("button", { name: "Download again" }));
    await waitFor(() => expect(fake.count("add")).toBe(1));
    expect(fake.lastAddOptions).toEqual({ onDuplicate: "allow" });
  });

  it("adds with refuse, and a concurrent duplicate shows the existing job instead", async () => {
    const fake = streaming([]);
    const url = "https://h.example/new.zip";
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByText(/No downloads/);
    // Between the check and the add, another client adds the same link.
    fake.next("add", async () => {
      fake.duplicates.set(url, [
        { job_id: "job_x", display_name: "new.zip", phase: "queued", desired_state: "running" },
      ]);
      throw callError("rejected", "DUPLICATE_DOWNLOAD", "none");
    });
    await user.click(command("Add URL"));
    await user.type(screen.getByLabelText("Address (URL)"), url);
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByText("This link is already in the list")).toBeVisible();
    expect(fake.lastAddOptions).toEqual({ onDuplicate: "refuse" });
    expect(fake.count("add")).toBe(1);
  });

  it("repeats an uncertain add with the same key and options", async () => {
    const fake = streaming([]);
    const url = "https://h.example/u.pdf";
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByText(/No downloads/);
    fake.next("add", async () => {
      throw callError("uncertain");
    });
    await user.click(command("Add URL"));
    await user.type(screen.getByLabelText("Address (URL)"), url);
    await user.selectOptions(screen.getByLabelText("Save as"), "documents");
    await user.click(screen.getByRole("button", { name: "Add" }));
    await screen.findByText(/may already have been added/i);
    await user.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fake.count("add")).toBe(2));
    const [first, second] = fake.calls.filter((c) => c.method === "add");
    expect(second?.args).toEqual(first?.args);
    expect(fake.lastAddOptions).toEqual({ onDuplicate: "refuse", category: "documents" });
    // The retry was not preceded by a second duplicate check.
    expect(fake.count("check")).toBe(1);
  });

  it("asks before saving under a new name and never overwrites", async () => {
    const fake = streaming([
      snapshot({
        job_id: "job_n",
        display_name: "report (1).pdf",
        phase: "verifying",
        blockers: ["name_conflict"],
      }),
    ]);
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "report (1).pdf" }));
    expect(screen.getByText("A file with this name already exists")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Save as report (1).pdf" }));
    await waitFor(() => expect(fake.count("confirmName")).toBe(1));
    expect(screen.queryByText("A file with this name already exists")).not.toBeInTheDocument();
  });

  it("saves download folders and the collision policy through the service", async () => {
    const fake = streaming([]);
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByText(/No downloads/);
    await user.click(screen.getByRole("button", { name: "Options" }));
    const docs = await screen.findByLabelText("Documents");
    await user.type(docs, "D:\\Docs");
    await user.selectOptions(screen.getByLabelText("When a file with the same name exists"), "ask");
    await user.click(screen.getByRole("button", { name: "Save download settings" }));
    expect(await screen.findByText("Download settings saved.")).toBeVisible();
    expect(fake.settings).toEqual({
      destination_rules: [{ category: "documents", directory: "D:\\Docs" }],
      on_collision: "ask",
    });
  });

  it("shows the service's refusal of a folder", async () => {
    const fake = streaming([]);
    fake.next("settingsSet", async () => {
      throw callError("rejected", "INVALID_REQUEST", "none");
    });
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByText(/No downloads/);
    await user.click(screen.getByRole("button", { name: "Options" }));
    await user.type(await screen.findByLabelText("Video"), "relative/path");
    await user.click(screen.getByRole("button", { name: "Save download settings" }));
    expect(await screen.findByText("The download settings were not saved.")).toBeVisible();
  });
});

describe("queues and priority (CORE-05)", () => {
  it("lists service queues and starts or stops the selected one", async () => {
    const fake = streaming([snapshot({ job_id: "job_q", display_name: "q.bin", queue_id: "q_1" })]);
    fake.queues.push({
      queue_id: "q_1",
      name: "Night",
      position: 1,
      max_active: 1,
      running: true,
    });
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "Night" }));
    expect(screen.getByRole("button", { name: "q.bin" })).toBeVisible();
    expect(command("Start queue")).toHaveAccessibleDescription(/Night is already running/);
    await user.click(command("Stop queue"));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "queueStop")).toEqual([
        { method: "queueStop", args: ["q_1"] },
      ]),
    );
    expect(await screen.findByRole("button", { name: "Night (stopped)" })).toBeVisible();
    expect(command("Stop queue")).toHaveAccessibleDescription(/Night is already stopped/);
  });

  it("shows a queue in execution order and moves a job with the keyboard", async () => {
    const fake = streaming([
      snapshot({ job_id: "job_2", display_name: "second.bin", queue_position: "2" }),
      snapshot({ job_id: "job_1", display_name: "first.bin", queue_position: "1" }),
    ]);
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "Main download queue" }));
    const names = () =>
      screen
        .getAllByRole("row")
        .slice(1)
        .map((r) => r.textContent ?? "");
    expect(names()[0]).toMatch(/first\.bin/);
    await user.click(screen.getByRole("button", { name: "second.bin" }));
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "move")).toEqual([
        { method: "move", args: ["job_2", "up"] },
      ]),
    );
  });

  it("changes a job's queue and priority, and manages queues without deleting downloads", async () => {
    const fake = streaming([snapshot({ job_id: "job_m", display_name: "m.bin" })]);
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "m.bin" });
    await menu(user, "Downloads", "Manage queues…");
    let dialog = screen.getByRole("dialog", { name: "Manage queues" });
    await user.type(within(dialog).getByLabelText("New queue name"), "Evening");
    await user.click(within(dialog).getByRole("button", { name: "Add queue" }));
    await within(dialog).findByText("Saved.");
    expect(fake.queues.map((q) => q.name)).toEqual(["Main queue", "Evening"]);
    await user.click(within(dialog).getByRole("button", { name: "Done" }));

    await user.click(screen.getByRole("button", { name: "m.bin" }));
    await menu(user, "File", "Queue and priority…");
    dialog = screen.getByRole("dialog", { name: "Queue and priority" });
    await user.selectOptions(within(dialog).getByLabelText("Priority"), "high");
    await waitFor(() => expect(fake.count("setPriority")).toBe(1));
    await user.selectOptions(within(dialog).getByLabelText("Queue"), "q_1");
    await waitFor(() => expect(fake.count("setQueue")).toBe(1));
    await user.click(within(dialog).getByRole("button", { name: "Done" }));

    // Deleting the non-empty queue moves its download back; nothing is deleted.
    await menu(user, "Downloads", "Manage queues…");
    dialog = screen.getByRole("dialog", { name: "Manage queues" });
    const evening = within(dialog).getByRole("listitem", { name: /Evening/ });
    await user.click(within(evening).getByRole("button", { name: "Delete queue" }));
    await within(dialog).findByText("Saved.");
    expect(fake.calls.find((c) => c.method === "queueDelete")?.args).toEqual(["q_1", "main"]);
    expect(fake.items.map((d) => d.job_id)).toEqual(["job_m"]);
    expect(fake.items[0]?.queue_id).toBe("main");
  });
});

describe("B2.7 behaviour on the event stream", () => {
  it("keeps search, selection and Clear finished while updates arrive as events", async () => {
    const fake = streaming([
      snapshot({ job_id: "job_d", display_name: "done.zip", phase: "completed" }),
      snapshot({ job_id: "job_r", display_name: "running.zip" }),
    ]);
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "running.zip" }));
    await user.type(screen.getByRole("searchbox", { name: "Search downloads" }), "zip");
    // A progress event for the selected row keeps it selected and visible.
    act(() => {
      fake.change(
        snapshot({ job_id: "job_r", display_name: "running.zip", downloaded_bytes: "1024" }),
        "progress",
      );
    });
    expect(screen.getByRole("button", { name: "running.zip" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await menu(user, "File", "Clear finished…");
    const dialog = screen.getByRole("dialog", { name: "Clear finished downloads?" });
    await user.click(within(dialog).getByRole("button", { name: "Clear finished" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "done.zip" })).not.toBeInTheDocument(),
    );
    // The service announces the removal too; a late update for it is ignored.
    act(() => {
      fake.emit("job_removed", { ...snapshot({ job_id: "job_d" }), removed: true });
      fake.emit(
        "job_changed",
        snapshot({ job_id: "job_d", display_name: "done.zip", phase: "completed" }),
      );
    });
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByRole("button", { name: "done.zip" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "running.zip" })).toBeVisible();
    expect(fake.count("list")).toBe(0);
  });
});
