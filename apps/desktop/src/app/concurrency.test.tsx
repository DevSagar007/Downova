import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, HELLO, callError, snapshot } from "../test/fake-service";

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

async function open(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  return within(screen.getByRole("dialog", { name: "Options" }));
}

describe("global download concurrency", () => {
  it("loads service truth and saves six once without silently changing a queue", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    expect(field).toHaveAccessibleDescription(/queue.*Manage queues/i);
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    expect(fake.count("concurrencySet")).toBe(1);
    expect(fake.count("queueLimit")).toBe(0);
  });

  it("rejects empty, fractional and out-of-range input without submitting", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    for (const value of ["", "0", "17", "2.5"]) {
      await user.clear(field);
      if (value) await user.type(field, value);
      await user.click(dialog.getByRole("button", { name: "Save download limit" }));
      expect(field).toHaveAttribute("aria-invalid", "true");
    }
    expect(fake.count("concurrencySet")).toBe(0);
  });

  it("holds one pending submission and keeps an uncertain outcome explicit", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let reject!: (e: unknown) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((_, no) => {
          reject = no;
        }),
    );
    await user.clear(field);
    await user.type(field, "10");
    await user.dblClick(dialog.getByRole("button", { name: "Save download limit" }));
    expect(fake.count("concurrencySet")).toBe(1);
    expect(field).toBeDisabled();
    await act(async () => reject(callError("uncertain")));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not confirm/i);
    expect(field).toHaveValue(10);
    fake.concurrency = { max_active_downloads: 10 };
    let resolveRead!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencyGet",
      () =>
        new Promise((resolve) => {
          resolveRead = resolve;
        }),
    );
    await user.click(dialog.getByRole("button", { name: "Reload download limit" }));
    expect(field).toBeDisabled();
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(fake.count("concurrencySet")).toBe(1);
    await act(async () => resolveRead({ max_active_downloads: 10 }));
    await waitFor(() => expect(dialog.queryByRole("alert")).not.toBeInTheDocument());
    expect(field).toHaveValue(10);
    expect(fake.count("concurrencySet"), "reading never repeats a save").toBe(1);
  });

  it("a rejected save never reports the rejected value as saved", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    fake.next("concurrencySet", () => Promise.reject(callError("rejected", "INTERNAL", "retry")));
    await user.clear(field);
    await user.type(field, "9");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not be read or saved/i);
    // The earlier "saved" status is gone and nothing claims 9 was saved.
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
    expect(fake.concurrency.max_active_downloads).toBe(6);
  });

  it("a rejected re-save of an unchanged value drops the earlier saved status", async () => {
    // Typing clears the status on its own; saving the same value again must clear it too.
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    fake.next("concurrencySet", () => Promise.reject(callError("rejected", "INTERNAL", "retry")));
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not be read or saved/i);
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("ignores a save reply from a service instance the window has since reconnected away from", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let resolveSave!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    await user.clear(field);
    await user.type(field, "12");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    // The service goes away and a new instance answers; the open dialog reads it afresh.
    fake.connection = { status: "unavailable", reason: "not_running", error: null };
    act(() => fake.endStream("lost"));
    await waitFor(() => expect(field).toBeDisabled());
    fake.connection = { status: "connected", hello: { ...HELLO, service_instance_id: "svc_new" } };
    fake.probeAnswer = fake.connection;
    await waitFor(() => expect(field).toHaveValue(3), { timeout: 5000 });
    // The old instance's reply arrives late: it must not replace what the new one reported.
    await act(async () => resolveSave({ max_active_downloads: 12 }));
    expect(field).toHaveValue(3);
    expect(field).toBeEnabled();
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("ignores a save reply that arrives after the dialog was closed and opened again", async () => {
    const { fake, user } = setup();
    let dialog = await open(user);
    let field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let resolveSave!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    await user.clear(field);
    await user.type(field, "12");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    await user.click(dialog.getByRole("button", { name: "Close" }));
    // Reopened: the dialog reads the service again (still 3), then the old reply lands.
    dialog = await open(user);
    field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await act(async () => resolveSave({ max_active_downloads: 12 }));
    expect(field).toHaveValue(3);
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("offers a read retry when the service setting cannot be loaded", async () => {
    const fake = new FakeServiceClient();
    fake.next("concurrencyGet", () => Promise.reject(callError("unavailable")));
    const { user } = setup(fake);
    const dialog = await open(user);
    await user.click(await dialog.findByRole("button", { name: "Reload download limit" }));
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    expect(fake.count("concurrencySet")).toBe(0);
  });

  it("localizes the label and validation in Bangla", async () => {
    const { user } = setup();
    const dialog = await open(user);
    await user.selectOptions(dialog.getByRole("combobox", { name: "Language" }), "bn");
    expect(
      await screen.findByRole("spinbutton", { name: "সর্বোচ্চ সক্রিয় ডাউনলোড" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "ডাউনলোডের সীমা সংরক্ষণ" })).toBeVisible();
  });
});

describe("what the saved limits allow (B2.31)", () => {
  /** The value shown next to a term in the explanation. */
  const valueOf = (dialog: ReturnType<typeof within>, term: string) =>
    dialog.getByText(term, { selector: "dt" }).nextElementSibling?.textContent ?? null;

  async function openWith(global: number, main: number, extra?: (fake: FakeServiceClient) => void) {
    const fake = new FakeServiceClient();
    fake.concurrency = { max_active_downloads: global };
    fake.queues = [
      { queue_id: "main", name: "Main queue", position: 0, max_active: main, running: true },
    ];
    extra?.(fake);
    const { user } = setup(fake);
    const dialog = await open(user);
    await dialog.findByRole("group", { name: "What this allows" });
    await waitFor(() => expect(valueOf(dialog, "Main queue limit")).toBe(String(main)));
    return { fake, user, dialog };
  }

  for (const [global, main, allowed, queueRestricts] of [
    [6, 6, 6, false],
    [8, 6, 6, true],
    [6, 3, 3, true],
  ] as const) {
    it(`global ${global}, Main ${main}: Main can run ${allowed}`, async () => {
      const { dialog } = await openWith(global, main);
      expect(valueOf(dialog, "Saved limit for all queues")).toBe(String(global));
      expect(valueOf(dialog, "Main can run at once")).toBe(String(allowed));
      expect(
        dialog.getByText(`The Main queue can run up to ${allowed} downloads at the same time.`),
      ).toBeVisible();
      expect(dialog.queryByText(/own limit \(\d+\) is lower/) !== null).toBe(queueRestricts);
      expect(dialog.queryByRole("button", { name: "Change queue limit" }) !== null).toBe(
        queueRestricts,
      );
    });
  }

  it("Change queue limit opens Manage queues; closing it returns to Options with the new limit", async () => {
    const { fake, user, dialog } = await openWith(6, 3);
    const change = dialog.getByRole("button", { name: "Change queue limit" });
    change.focus();
    await user.keyboard("{Enter}");
    const queues = within(await screen.findByRole("dialog", { name: "Manage queues" }));
    const limit = queues.getByRole("spinbutton", { name: "Downloads at once" });
    await user.clear(limit);
    await user.type(limit, "6");
    await user.click(queues.getByRole("button", { name: "Set limit" }));
    await waitFor(() => expect(fake.queues[0]?.max_active).toBe(6));
    await user.click(queues.getByRole("button", { name: "Close" }));
    const back = within(await screen.findByRole("dialog", { name: "Options" }));
    await waitFor(() => expect(valueOf(back, "Main queue limit")).toBe("6"));
    expect(valueOf(back, "Main can run at once")).toBe("6");
    expect(back.queryByRole("button", { name: "Change queue limit" })).not.toBeInTheDocument();
    expect(fake.concurrency.max_active_downloads, "the global limit is untouched").toBe(6);
  });

  it("refreshes after a successful global save without changing the queue", async () => {
    const { fake, user, dialog } = await openWith(6, 6);
    const field = dialog.getByRole("spinbutton", { name: "Maximum active downloads" });
    await user.clear(field);
    await user.type(field, "8");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    await waitFor(() => expect(valueOf(dialog, "Saved limit for all queues")).toBe("8"));
    expect(valueOf(dialog, "Main can run at once")).toBe("6");
    expect(dialog.getByRole("button", { name: "Change queue limit" })).toBeVisible();
    expect(fake.count("queueLimit")).toBe(0);
  });

  it("explains the aria2 engine's cap when both limits are higher", async () => {
    const { dialog } = await openWith(6, 6, (fake) => {
      fake.connection = {
        status: "connected",
        hello: { ...HELLO, backend: { ...HELLO.backend, id: "aria2" } },
      };
    });
    expect(valueOf(dialog, "Main can run at once")).toBe("3");
    expect(
      dialog.getByText(
        "The aria2 download engine runs at most 3 files at once, so Main can run 3 even though both limits are higher.",
      ),
    ).toBeVisible();
  });

  it("separates capacity from what is active now, a stopped Main and more active than the limit", async () => {
    const running = (i: number, queue = "main") =>
      snapshot({
        job_id: `job_0000000000000${i}a`,
        display_name: `r${i}.bin`,
        attempt_id: `att_000000000000${i}a`,
        phase: "transferring",
        queue_id: queue,
      });
    const { dialog } = await openWith(2, 6, (fake) => {
      fake.queues = [
        { queue_id: "main", name: "Main queue", position: 0, max_active: 6, running: false },
        { queue_id: "q_other", name: "Other", position: 1, max_active: 6, running: true },
      ];
      fake.items = [
        running(1),
        running(2),
        running(3, "q_other"),
        // Waiting and paused jobs do not use a slot.
        // An attempt held by the disk policy does not either (the service's is_active).
        snapshot({
          job_id: "job_00000000000006a",
          attempt_id: "att_0000000000006a",
          phase: "transferring",
          blockers: ["disk_unavailable"],
        }),
        snapshot({ job_id: "job_00000000000004a", phase: "queued", blockers: ["queue_limit"] }),
        snapshot({
          job_id: "job_00000000000005a",
          attempt_id: "att_0000000000005a",
          desired_state: "paused",
        }),
      ];
    });
    expect(valueOf(dialog, "Active now")).toBe("3 in all queues, 2 in Main");
    expect(dialog.getByText(/The Main queue is stopped/)).toBeVisible();
    expect(dialog.getByText(/More downloads are active than the limit/)).toBeVisible();
    expect(dialog.getByText(/Downloads in other queues count toward/)).toBeVisible();
  });

  it("reads the queue limits again after reconnecting to a new service instance", async () => {
    const { fake, dialog } = await openWith(6, 3);
    fake.connection = { status: "unavailable", reason: "not_running", error: null };
    act(() => fake.endStream("lost"));
    await waitFor(() =>
      expect(dialog.queryByRole("group", { name: "What this allows" })).not.toBeInTheDocument(),
    );
    fake.queues = [
      { queue_id: "main", name: "Main queue", position: 0, max_active: 6, running: true },
    ];
    fake.connection = { status: "connected", hello: { ...HELLO, service_instance_id: "svc_new" } };
    fake.probeAnswer = fake.connection;
    await waitFor(() => expect(valueOf(dialog, "Main queue limit")).toBe("6"), { timeout: 5000 });
    expect(valueOf(dialog, "Main can run at once")).toBe("6");
  });

  it("explains the limits in Bangla", async () => {
    const { user } = await openWith(6, 3);
    await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "bn");
    const group = within(await screen.findByRole("group", { name: "এতে যা চলতে পারে" }));
    expect(group.getByText("প্রধান কিউ একই সময়ে সর্বোচ্চ 3টি ডাউনলোড চালাতে পারে।")).toBeVisible();
    expect(group.getByRole("button", { name: "কিউর সীমা বদলান" })).toBeVisible();
  });
});
