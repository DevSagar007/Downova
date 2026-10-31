// B2.1: the window driven by a fake ServiceClient — live list, Add URL, selection, Stop/Resume,
// blocked and failed jobs, disconnect/reconnect and stale polling answers.
import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, deferred, FakeServiceClient, HELLO, snapshot } from "../test/fake-service";

function renderLive(fake: FakeServiceClient, pollIntervalMs = 20) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={fake} pollIntervalMs={pollIntervalMs} reconnectDelaysMs={[20, 20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

const toolbar = () => screen.getByRole("toolbar");
const command = (name: string) => within(toolbar()).getByRole("button", { name });
const row = (name: RegExp) => screen.getByRole("row", { name });
const select = (name: string) => screen.getByRole("button", { name });

describe("live download list", () => {
  it("shows loading, then the service's snapshots with real progress", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_a",
        display_name: "a.iso",
        downloaded_bytes: "1048576",
        total_bytes: "4194304",
      }),
      snapshot({
        job_id: "job_b",
        display_name: "b.bin",
        downloaded_bytes: "1536",
        total_bytes: null,
      }),
    ];
    const first = deferred<DownloadSnapshot[]>();
    fake.next("list", () => first.promise);
    renderLive(fake);
    expect(await screen.findByText("Loading downloads…")).toBeVisible();
    await act(async () => first.resolve(fake.items));
    expect(within(row(/a\.iso/)).getByText("Downloading 25.0%")).toBeVisible();
    // Unknown total: the received bytes, never an invented percentage, rate or ETA.
    expect(within(row(/b\.bin/)).getByText("Downloading 1.5 KB")).toBeVisible();
    expect(within(row(/b\.bin/)).getAllByText("—")).toHaveLength(3);
    expect(screen.getByText("2 downloads")).toBeVisible();
  });

  it("keeps the row and its selection when the service renames the file", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_n", display_name: "get" })];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "get" }));
    fake.items = [snapshot({ job_id: "job_n", display_name: "report.pdf" })];
    expect(await screen.findByRole("button", { name: "report.pdf" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("button", { name: "get" })).not.toBeInTheDocument();
  });

  it("shows completed verification instead of inventing it", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_v",
        display_name: "v.zip",
        phase: "completed",
        downloaded_bytes: "10",
        total_bytes: "10",
        verification: { sha256: "ab".repeat(32), source: "local_hash" },
      }),
    ];
    renderLive(fake);
    const done = await screen.findByRole("row", { name: /v\.zip/ });
    expect(within(done).getByText("Complete")).toBeVisible();
    // A locally computed hash is never presented as a publisher verification (B2.18).
    expect(
      within(done).getByText("SHA-256 computed locally (no publisher checksum)"),
    ).toBeVisible();
  });
});

describe("Add URL", () => {
  async function openDialog(user: ReturnType<typeof userEvent.setup>) {
    await waitFor(() => expect(command("Add URL")).not.toHaveAttribute("aria-disabled"));
    await user.click(command("Add URL"));
    return screen.getByRole("dialog", { name: "Add new download" });
  }

  it("explains invalid links and sends nothing", async () => {
    const fake = new FakeServiceClient();
    const user = userEvent.setup();
    renderLive(fake);
    const dialog = await openDialog(user);
    const input = within(dialog).getByLabelText("Address (URL)");
    expect(input).toHaveFocus();
    for (const [link, message] of [
      ["ftp://h/f", "Only links that start with http:// or https:// can be added."],
      ["https://user:pw@h/f", /user name or password/],
      ["https://h/a b", "The link contains spaces or control characters."],
      ["https:///f", "The link has no server name."],
    ] as const) {
      await user.clear(input);
      await user.type(input, link);
      await user.click(within(dialog).getByRole("button", { name: "Add" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(message);
      expect(input).toHaveAttribute("aria-invalid", "true");
    }
    await user.clear(input);
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Enter a link.");
    expect(fake.count("add")).toBe(0);
  });

  it("submits once, even when Add is pressed repeatedly, then selects the new job", async () => {
    const fake = new FakeServiceClient();
    const ack = deferred<DownloadSnapshot>();
    fake.next("add", () => ack.promise);
    const user = userEvent.setup();
    renderLive(fake);
    const dialog = await openDialog(user);
    await user.type(within(dialog).getByLabelText("Address (URL)"), "https://example.org/f.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    const adding = within(dialog).getByRole("button", { name: "Adding…" });
    expect(adding).toHaveAttribute("aria-disabled", "true");
    await user.click(adding);
    await user.type(within(dialog).getByLabelText("Address (URL)"), "{Enter}");
    expect(fake.count("add")).toBe(1);
    const job = snapshot({ job_id: "job_f", display_name: "f.iso", phase: "queued" });
    fake.items = [job];
    await act(async () => ack.resolve(job));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "f.iso" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("retries an unconfirmed add only on request, with the same key", async () => {
    const fake = new FakeServiceClient();
    fake.next("add", async () => {
      throw callError("uncertain");
    });
    const user = userEvent.setup();
    renderLive(fake);
    const dialog = await openDialog(user);
    const input = within(dialog).getByLabelText("Address (URL)");
    await user.type(input, "https://example.org/f.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      /may already have been added/,
    );
    // Nothing is replayed automatically.
    await new Promise((r) => setTimeout(r, 60));
    expect(fake.count("add")).toBe(1);
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const [first, second] = fake.calls.filter((c) => c.method === "add");
    expect(second?.args).toEqual(first?.args);
    expect(first?.args[1]).toMatch(/^ui-[A-Za-z0-9-]{8,}$/);
  });

  it("keeps the unconfirmed link and key when the dialog is closed and reopened", async () => {
    const fake = new FakeServiceClient();
    fake.next("add", async () => {
      throw callError("uncertain");
    });
    const user = userEvent.setup();
    renderLive(fake);
    let dialog = await openDialog(user);
    await user.type(within(dialog).getByLabelText("Address (URL)"), "https://example.org/k.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await within(dialog).findByRole("alert");
    await user.keyboard("{Escape}");
    dialog = await openDialog(user);
    expect(within(dialog).getByLabelText("Address (URL)")).toHaveValue("https://example.org/k.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const [first, second] = fake.calls.filter((c) => c.method === "add");
    expect(second?.args).toEqual(first?.args);
    // Once confirmed, the next add starts empty with a fresh key.
    dialog = await openDialog(user);
    expect(within(dialog).getByLabelText("Address (URL)")).toHaveValue("");
  });

  it("uses a new key for a different link and shows a service refusal", async () => {
    const fake = new FakeServiceClient();
    fake.next("add", async () => {
      throw callError("rejected", "BACKEND_CAPABILITY_UNAVAILABLE", "none");
    });
    const user = userEvent.setup();
    renderLive(fake);
    const dialog = await openDialog(user);
    const input = within(dialog).getByLabelText("Address (URL)");
    await user.type(input, "https://example.org/a.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("The download service did not add this link.");
    expect(alert).toHaveTextContent("The download engine cannot do this safely.");
    await user.clear(input);
    await user.type(input, "https://example.org/b.iso");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fake.count("add")).toBe(2));
    const [a, b] = fake.calls.filter((c) => c.method === "add");
    expect(a?.args[1]).not.toBe(b?.args[1]);
  });

  it("closes with Escape and returns focus to Add URL", async () => {
    const fake = new FakeServiceClient();
    const user = userEvent.setup();
    renderLive(fake);
    await openDialog(user);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(command("Add URL")).toHaveFocus();
  });
});

describe("selection, Stop and Resume", () => {
  it("pauses and resumes the selected download through the service", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_s", display_name: "s.bin" })];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "s.bin" });
    expect(command("Stop")).toHaveAccessibleDescription(/select a download/i);
    await user.click(select("s.bin"));
    expect(command("Resume")).toHaveAccessibleDescription(/not paused/i);
    await user.click(command("Stop"));
    expect(fake.calls.find((c) => c.method === "pause")?.args).toEqual(["job_s"]);
    expect(await within(row(/s\.bin/)).findByText("Paused")).toBeVisible();
    expect(command("Stop")).toHaveAccessibleDescription(/already paused/i);
    await user.click(command("Resume"));
    expect(fake.calls.find((c) => c.method === "resume")?.args).toEqual(["job_s"]);
    await waitFor(() => expect(within(row(/s\.bin/)).queryByText("Paused")).toBeNull());
  });

  it("offers the same commands in the File menu", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_m", display_name: "m.bin" })];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "m.bin" }));
    await user.click(screen.getByRole("menuitem", { name: "File" }));
    await user.click(screen.getByRole("menuitem", { name: "Stop download" }));
    await waitFor(() => expect(fake.count("pause")).toBe(1));
  });

  it("moves the selection with the arrow keys", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_1", display_name: "one.bin" }),
      snapshot({ job_id: "job_2", display_name: "two.bin" }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "one.bin" }));
    await user.keyboard("{ArrowDown}");
    expect(select("two.bin")).toHaveFocus();
    expect(select("two.bin")).toHaveAttribute("aria-pressed", "true");
    expect(select("one.bin")).toHaveAttribute("aria-pressed", "false");
    await user.keyboard("{ArrowUp}");
    expect(select("one.bin")).toHaveAttribute("aria-pressed", "true");
  });

  it("shows why a command failed, using the error catalogue", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_r", display_name: "r.bin", desired_state: "paused" })];
    fake.next("resume", async () => {
      throw callError("rejected", "BACKEND_CAPABILITY_UNAVAILABLE", "choose_restart_or_keep");
    });
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "r.bin" }));
    await user.click(command("Resume"));
    expect(await screen.findByRole("heading", { name: "Resume did not complete" })).toBeVisible();
    expect(screen.getByText(/saved part cannot be continued safely/)).toBeVisible();
  });
});

describe("blocked and failed downloads", () => {
  it("keeps a policy wait distinct from a manual pause and says it continues by itself", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_w",
        display_name: "w.bin",
        phase: "queued",
        blockers: ["disk_unavailable"],
        error: { kind: "DISK_FULL", retryable: true, next_action: "free_disk_space", detail: null },
      }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "w.bin" }));
    expect(within(row(/w\.bin/)).getByText("Waiting: disk unavailable")).toBeVisible();
    expect(command("Resume")).toHaveAccessibleDescription(/continues by itself/);
    expect(command("Stop")).not.toHaveAttribute("aria-disabled");
    expect(
      screen.getByRole("heading", {
        name: "The destination drive does not have enough free space.",
      }),
    ).toBeVisible();
    expect(screen.getByText("Free up space on the destination drive.")).toBeVisible();
    expect(screen.getByText("The download continues by itself once this is fixed.")).toBeVisible();
  });

  it("never offers to resume a failed download", async () => {
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_x",
        display_name: "x.bin",
        phase: "failed",
        error: {
          kind: "LINK_REFRESH_REQUIRED",
          retryable: false,
          next_action: "refresh_link",
          detail: "resource no longer available (HTTP 410)",
        },
      }),
    ];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "x.bin" }));
    expect(within(row(/x\.bin/)).getByText("Error")).toBeVisible();
    expect(command("Resume")).toHaveAttribute("aria-disabled", "true");
    expect(command("Resume")).toHaveAccessibleDescription(/cannot be resumed/);
    expect(command("Stop")).toHaveAccessibleDescription(/stopped with an error/);
    expect(screen.getByText("Get a new link from the website and use Refresh link.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Refresh link…" })).toBeVisible();
    // The service's English detail is not shown as the explanation.
    expect(screen.queryByText(/HTTP 410/)).not.toBeInTheDocument();
  });
});

describe("connection", () => {
  it("keeps the last list when the service goes away and reconnects on request", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_k", display_name: "k.bin" })];
    const user = userEvent.setup();
    renderLive(fake);
    await screen.findByRole("button", { name: "k.bin" });
    fake.connection = { status: "unavailable", reason: "not_running", error: null };
    expect(await screen.findByText(/Showing the last known list/)).toBeVisible();
    expect(select("k.bin")).toBeVisible();
    expect(command("Add URL")).toHaveAccessibleDescription(/not connected/i);
    const connects = fake.count("connect");
    const lists = fake.count("list");
    await new Promise((r) => setTimeout(r, 80));
    expect(fake.count("list")).toBe(lists);
    // Automatic reconnection only looks for a running service (two bounded probes), never
    // starts one; afterwards the user's Reconnect is offered (B2.10).
    const reconnect = await screen.findByRole("button", { name: "Reconnect" });
    expect(fake.count("probe")).toBe(2);
    expect(fake.count("connect")).toBe(connects);
    fake.connection = { status: "connected", hello: HELLO };
    fake.items = [snapshot({ job_id: "job_k", display_name: "k.bin", phase: "completed" })];
    await user.click(reconnect);
    expect(fake.count("connect")).toBeGreaterThan(connects);
    expect(await within(row(/k\.bin/)).findByText("Complete")).toBeVisible();
    expect(screen.queryByText(/Showing the last known list/)).not.toBeInTheDocument();
  });

  it("drops a list answer that started before an acknowledged Stop", async () => {
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_p", display_name: "p.bin" })];
    const user = userEvent.setup();
    renderLive(fake);
    await user.click(await screen.findByRole("button", { name: "p.bin" }));
    const late = deferred<DownloadSnapshot[]>();
    const before = fake.count("list");
    fake.next("list", () => late.promise);
    await waitFor(() => expect(fake.count("list")).toBe(before + 1));
    await user.click(command("Stop"));
    expect(await within(row(/p\.bin/)).findByText("Paused")).toBeVisible();
    // The poll that was in flight describes the job before Stop: it must not undo it.
    await act(async () => late.resolve([snapshot({ job_id: "job_p", display_name: "p.bin" })]));
    expect(within(row(/p\.bin/)).getByText("Paused")).toBeVisible();
    await waitFor(() => expect(fake.count("list")).toBeGreaterThan(before + 1));
    expect(within(row(/p\.bin/)).getByText("Paused")).toBeVisible();
  });

  it("stops polling when the window unmounts", async () => {
    const fake = new FakeServiceClient();
    const view = renderLive(fake);
    await waitFor(() => expect(fake.count("list")).toBeGreaterThan(1));
    view.unmount();
    const after = fake.count("list");
    await new Promise((r) => setTimeout(r, 80));
    expect(fake.count("list")).toBe(after);
  });

  it("explains an incompatible service without offering to replace it", async () => {
    const fake = new FakeServiceClient();
    fake.connection = { status: "unavailable", reason: "incompatible_version", error: null };
    renderLive(fake);
    expect(
      await screen.findByRole("heading", {
        name: "A different version of the download service is running",
      }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    expect(command("Add URL")).toHaveAccessibleDescription(/different version/);
    expect(fake.count("list")).toBe(0);
  });
});

describe("Bangla", () => {
  it("shows the dialog, statuses and command reasons in Bangla", async () => {
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ locale: "bn" }));
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_b", display_name: "বই.pdf", desired_state: "paused" })];
    const user = userEvent.setup();
    renderLive(fake);
    expect(
      await within(await screen.findByRole("row", { name: /বই\.pdf/ })).findByText("থামানো"),
    ).toBeVisible();
    expect(command("থামান")).toHaveAccessibleDescription(/নির্বাচন করুন/);
    await user.click(command("URL যোগ"));
    const dialog = screen.getByRole("dialog", { name: "নতুন ডাউনলোড যোগ করুন" });
    await user.type(within(dialog).getByLabelText("ঠিকানা (URL)"), "ftp://h/f");
    await user.click(within(dialog).getByRole("button", { name: "যোগ করুন" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("http:// বা https://");
  });
});
