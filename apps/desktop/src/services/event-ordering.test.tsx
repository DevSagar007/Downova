// Event batching and ordering in ServiceProvider (ARCH-08, ADV12; B2.24 hardening). These checks
// read the provider's state directly: what matters is which snapshot wins, not how it is drawn.
import { useEffect } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DownloadSnapshot } from "@dm/contracts";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  HIDDEN_FLUSH_MS,
  RESUBSCRIBE_BACKOFF_MS,
  RESUBSCRIBE_FREE,
  ServiceProvider,
  useDownloads,
  useServiceActions,
  VISIBLE_FLUSH_MS,
  type DownloadsState,
  type ServiceActions,
} from "./ServiceProvider";
import { ServiceCallError } from "./service-client";
import { App } from "../app/App";
import { PreferencesProvider } from "../app/preferences";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";

// What the provider last committed (written after each commit, read by the checks).
const probe: { seen: DownloadsState | null; actions: ServiceActions | null } = {
  seen: null,
  actions: null,
};
function Probe() {
  const seen = useDownloads();
  const actions = useServiceActions();
  useEffect(() => {
    probe.seen = seen;
    probe.actions = actions;
  });
  return null;
}
const state = () => {
  if (!probe.seen) throw new Error("not rendered");
  return probe.seen;
};
const act$ = () => {
  if (!probe.actions) throw new Error("not rendered");
  return probe.actions;
};
const job = (id: string) => state().items.find((d) => d.job_id === id);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** Longer than a visible flush, shorter than a hidden one. */
const VISIBLE_WAIT = VISIBLE_FLUSH_MS * 3;

function live(items: DownloadSnapshot[]) {
  const fake = new FakeServiceClient();
  fake.streaming = true;
  fake.items = items;
  const view = render(
    <ServiceProvider
      client={fake}
      shell={new FakeShell()}
      pollIntervalMs={20}
      reconnectDelaysMs={[20, 20]}
    >
      <Probe />
    </ServiceProvider>,
  );
  return { fake, view };
}

function setVisibility(v: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => v });
  document.dispatchEvent(new Event("visibilitychange"));
}

afterEach(() => {
  setVisibility("visible");
  probe.seen = null;
  probe.actions = null;
});

const running = (over: Partial<DownloadSnapshot> = {}) =>
  snapshot({ job_id: "job_a", display_name: "a.bin", attempt_id: "att_1", ...over });

describe("event batches against acknowledgements", () => {
  it("applies progress queued just before an acknowledgement first, so the acknowledgement wins", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    // Progress arrives and is queued (not yet applied)...
    act(() => fake.emit("progress", running({ downloaded_bytes: "500" })));
    expect(job("job_a")?.downloaded_bytes).toBe("0");
    // ...and the pause acknowledgement lands before the flush.
    await act(async () => {
      await act$().pause("job_a");
    });
    expect(job("job_a")?.desired_state).toBe("paused");
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")?.desired_state).toBe("paused");
  });

  for (const [name, start, run] of [
    ["resume", running({ desired_state: "paused" }), () => act$().resume("job_a")],
    [
      "retry",
      running({ phase: "failed", desired_state: "running" }),
      () => act$().retry("job_a", "op_1", { expectedAttemptId: "att_1", discardPartial: true }),
    ],
    [
      "replace source",
      running({ phase: "failed" }),
      () =>
        act$().replaceSource("job_a", "op_2", "https://example.org/new.bin", "restart", "att_1"),
    ],
  ] as const) {
    it(`keeps the ${name} acknowledgement over an older queued event`, async () => {
      const { fake } = live([start]);
      await waitFor(() => expect(job("job_a")).toBeDefined());
      act(() => fake.emit("job_changed", { ...start, downloaded_bytes: "7" }));
      let acked: DownloadSnapshot | null = null;
      await act(async () => {
        const r = await run();
        acked = "job" in r ? r.job : r;
      });
      await act(() => sleep(VISIBLE_WAIT));
      expect(job("job_a")).toEqual(acked);
    });
  }

  it("a removal acknowledged while events are queued keeps the job out for good", async () => {
    const done = running({ phase: "completed" });
    const { fake } = live([done]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => fake.emit("job_changed", done));
    await act(async () => {
      await act$().remove("job_a");
    });
    expect(job("job_a")).toBeUndefined();
    // Late events for it, and a later snapshot that still lists it (an older service view).
    act(() => fake.emit("job_changed", done));
    await act(() => sleep(VISIBLE_WAIT));
    fake.items = [done];
    act(() => fake.endStream("lost"));
    await waitFor(() => expect(fake.subscriptions).toBe(2));
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")).toBeUndefined();
  });

  it("an event emitted before a mutation but delivered after its acknowledgement is corrected by the mutation's own event", async () => {
    // The stream and the acknowledgement travel on different connections, so a stale event can
    // land after the acknowledgement. The service always announces the mutation itself next in
    // its stream; within one flush window both coalesce, and the final state is the mutation's.
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    let paused: DownloadSnapshot | null = null;
    await act(async () => {
      paused = await act$().pause("job_a");
    });
    act(() => {
      fake.emit("progress", running({ downloaded_bytes: "900" })); // stale
      if (paused) fake.emit("job_changed", paused); // the mutation's own event
    });
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")).toEqual(paused);
  });
});

describe("subscriptions and snapshots", () => {
  it("ignores events and endings from a superseded subscription", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(fake.subscriptions).toBe(1));
    const old = fake.allHandlers[0];
    act(() => fake.endStream("lost"));
    await waitFor(() => expect(fake.subscriptions).toBe(2));
    const nextSeq = (fake.sequence + 1n).toString();
    act(() => {
      old?.onEvent({
        service_instance_id: fake.instance,
        sequence: nextSeq,
        kind: "job_changed",
        job_id: "job_a",
        attempt_id: "att_1",
        snapshot: running({ phase: "failed" }),
      });
      old?.onEnded("lost");
    });
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")?.phase).toBe("transferring");
    expect(fake.subscriptions).toBe(2);
  });

  it("drops a queued batch when a fresh snapshot replaces the stream", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => fake.emit("progress", running({ downloaded_bytes: "100" })));
    // The service moved on; a new instance answers with its authoritative list.
    fake.items = [running({ phase: "completed", downloaded_bytes: "1000" })];
    fake.instance = "svc_new";
    act(() => fake.emit("progress", running({ downloaded_bytes: "200" })));
    await waitFor(() => expect(fake.subscriptions).toBe(2));
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")?.phase).toBe("completed");
    expect(job("job_a")?.downloaded_bytes).toBe("1000");
  });

  it("a sequence gap with a batch queued ends with the new snapshot, not the batch", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => fake.emit("progress", running({ downloaded_bytes: "100" })));
    fake.items = [running({ downloaded_bytes: "300" })];
    act(() => fake.emit("progress", running({ downloaded_bytes: "999" }), fake.sequence + 5n));
    await waitFor(() => expect(fake.subscriptions).toBe(2));
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")?.downloaded_bytes).toBe("300");
  });
});

describe("window visibility and teardown", () => {
  it("holds a hidden window's batch for the hidden interval and applies it on becoming visible", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => setVisibility("hidden"));
    act(() => {
      for (let i = 1; i <= 50; i++) fake.emit("progress", running({ downloaded_bytes: String(i) }));
    });
    await act(() => sleep(VISIBLE_WAIT));
    expect(job("job_a")?.downloaded_bytes).toBe("0");
    act(() => setVisibility("visible"));
    expect(job("job_a")?.downloaded_bytes).toBe("50");
  });

  it("a hidden window still applies its batch after the hidden interval", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => setVisibility("hidden"));
    act(() => fake.emit("progress", running({ downloaded_bytes: "5" })));
    await act(() => sleep(HIDDEN_FLUSH_MS + 100));
    expect(job("job_a")?.downloaded_bytes).toBe("5");
  });

  it("unmounting with a batch queued closes the stream and leaves no timer or listener behind", async () => {
    const added = vi.spyOn(document, "addEventListener");
    const removedL = vi.spyOn(document, "removeEventListener");
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const { fake, view } = live([running()]);
    await waitFor(() => expect(job("job_a")).toBeDefined());
    act(() => fake.emit("progress", running({ downloaded_bytes: "5" })));
    const before = probe.seen;
    view.unmount();
    await sleep(VISIBLE_WAIT);
    expect(fake.subscribed).toBe(false);
    expect(probe.seen).toBe(before); // nothing rendered after unmount
    const count = (spy: typeof added) =>
      spy.mock.calls.filter(([type]) => type === "visibilitychange").length;
    expect(count(removedL)).toBe(count(added));
    expect(errors).not.toHaveBeenCalled();
    added.mockRestore();
    removedL.mockRestore();
    errors.mockRestore();
  });

  it("repeated removal and reconnect cycles keep removed jobs out and one listener per stream", async () => {
    const added = vi.spyOn(document, "addEventListener");
    const removedL = vi.spyOn(document, "removeEventListener");
    const { fake } = live([]);
    await waitFor(() => expect(fake.subscriptions).toBe(1));
    for (let i = 0; i < 5; i++) {
      const id = `job_c${i}`;
      const done = snapshot({ job_id: id, display_name: `${id}.bin`, phase: "completed" });
      act(() => fake.change(done, "job_added"));
      await waitFor(() => expect(job(id)).toBeDefined());
      await act(async () => {
        await act$().remove(id);
      });
      act(() => fake.emit("job_changed", done)); // late
      act(() => fake.endStream("lost"));
      await waitFor(() => expect(fake.subscriptions).toBe(i + 2));
    }
    await act(() => sleep(VISIBLE_WAIT));
    expect(state().items).toEqual([]);
    const count = (spy: typeof added) =>
      spy.mock.calls.filter(([type]) => type === "visibilitychange").length;
    // The stream effect is set up once per connection; reconnects reuse it (one live listener).
    expect(count(added) - count(removedL)).toBe(1);
    added.mockRestore();
    removedL.mockRestore();
  });
});

describe("bounded failure (B2.24)", () => {
  it("backs off when streams keep ending without any event in between", async () => {
    const { fake } = live([running()]);
    await waitFor(() => expect(fake.subscriptions).toBe(1));
    // RESUBSCRIBE_FREE immediate restarts...
    for (let i = 0; i < RESUBSCRIBE_FREE; i++) {
      act(() => fake.endStream("lost"));
      await waitFor(() => expect(fake.subscriptions).toBe(i + 2));
    }
    // ...then the next one waits for the backoff.
    act(() => fake.endStream("lost"));
    await act(() => sleep(RESUBSCRIBE_BACKOFF_MS / 2));
    expect(fake.subscriptions).toBe(RESUBSCRIBE_FREE + 1);
    await waitFor(() => expect(fake.subscriptions).toBe(RESUBSCRIBE_FREE + 2), {
      timeout: RESUBSCRIBE_BACKOFF_MS * 3,
    });
    // An applied event resets the count: the next restart is immediate again.
    act(() => fake.change(running({ downloaded_bytes: "1" })));
    act(() => fake.endStream("lost"));
    await waitFor(() => expect(fake.subscriptions).toBe(RESUBSCRIBE_FREE + 3), { timeout: 200 });
  });

  it("an unreadable list stops in a reported state instead of polling or resubscribing", async () => {
    const fake = new FakeServiceClient();
    fake.streaming = true;
    fake.items = [running()];
    fake.next("subscribe", () =>
      Promise.reject(
        new ServiceCallError("rejected", {
          kind: "INTERNAL",
          retryable: false,
          next_action: "contact_support",
          detail: "the download service's answer is larger than this client accepts",
        }),
      ),
    );
    render(
      <PreferencesProvider>
        <ServiceProvider
          client={fake}
          shell={new FakeShell()}
          pollIntervalMs={20}
          reconnectDelaysMs={[20, 20]}
        >
          <App />
        </ServiceProvider>
      </PreferencesProvider>,
    );
    expect(await screen.findByText("The download list could not be loaded")).toBeVisible();
    await sleep(200);
    expect(fake.count("subscribe")).toBe(1);
    expect(fake.count("list")).toBe(0);
    expect(fake.count("probe")).toBe(0);
    // Only the user's Reconnect asks again.
    await userEvent.setup().click(screen.getByRole("button", { name: "Reconnect" }));
    expect(await screen.findByRole("button", { name: "a.bin" })).toBeVisible();
    expect(fake.count("subscribe")).toBe(2);
  });
});
