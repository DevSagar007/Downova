import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { HelloResult } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, HELLO, callError, snapshot } from "../test/fake-service";
import { bn, en } from "../i18n/messages";

/** The default fake reports no capabilities; this one supports both speed limits (CORE-06). */
function supporting(fake = new FakeServiceClient()) {
  const hello = {
    ...HELLO,
    backend: {
      ...HELLO.backend,
      capabilities: [
        { capability: "speed_limit_global", support: "supported" },
        { capability: "speed_limit_per_job", support: "supported" },
      ],
    },
  } as unknown as HelloResult;
  fake.connection = { status: "connected", hello };
  return fake;
}

function setup(fake = supporting()) {
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

async function options(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  return within(screen.getByRole("dialog", { name: "Options" }));
}

describe("overall speed limit (CORE-06)", () => {
  it("saves a limit in MB/s, shows the service's value, and returns to unlimited", async () => {
    const { fake, user } = setup();
    const dialog = await options(user);
    const box = await dialog.findByRole("checkbox", { name: "Limit the overall download speed" });
    await waitFor(() => expect(box).toBeEnabled());
    expect(box).not.toBeChecked();
    await user.click(box);
    const value = dialog.getByRole("textbox", { name: "Overall speed" });
    await user.type(value, "2");
    expect(dialog.getByRole("combobox", { name: "Unit" })).toHaveValue("mb");
    await user.click(dialog.getByRole("button", { name: "Save speed limit" }));
    expect(await dialog.findByRole("status", { name: "Speed limit" })).toHaveTextContent(
      "Saved overall limit: 2 MB/s",
    );
    expect(fake.calls.filter((c) => c.method === "speedLimitSet").map((c) => c.args)).toEqual([
      ["2000000"],
    ]);
    await user.click(box);
    await user.click(dialog.getByRole("button", { name: "Save speed limit" }));
    expect(await dialog.findByRole("status", { name: "Speed limit" })).toHaveTextContent(
      "Saved: no overall limit.",
    );
    expect(fake.speedLimit.global_bytes_per_second).toBeNull();
  });

  it("refuses values outside the contract's bounds without sending them", async () => {
    const { fake, user } = setup();
    const dialog = await options(user);
    const box = await dialog.findByRole("checkbox", { name: "Limit the overall download speed" });
    await waitFor(() => expect(box).toBeEnabled());
    await user.click(box);
    const value = dialog.getByRole("textbox", { name: "Overall speed" });
    for (const bad of ["0", "1.5.5", "abc", "1000001"]) {
      await user.clear(value);
      await user.type(value, bad);
      await user.click(dialog.getByRole("button", { name: "Save speed limit" }));
      expect(value).toHaveAttribute("aria-invalid", "true");
    }
    expect(fake.count("speedLimitSet")).toBe(0);
  });

  it("explains an engine that cannot limit speed and offers no limit", async () => {
    const { fake, user } = setup(new FakeServiceClient());
    const dialog = await options(user);
    const box = await dialog.findByRole("checkbox", { name: "Limit the overall download speed" });
    await waitFor(() =>
      expect(dialog.getByText("This download engine cannot limit speed.")).toBeVisible(),
    );
    expect(box).toBeDisabled();
    expect(fake.count("speedLimitSet")).toBe(0);
  });

  it("keeps an uncertain save explicit and reads the value back on reload", async () => {
    const { fake, user } = setup();
    const dialog = await options(user);
    const box = await dialog.findByRole("checkbox", { name: "Limit the overall download speed" });
    await waitFor(() => expect(box).toBeEnabled());
    await user.click(box);
    await user.type(dialog.getByRole("textbox", { name: "Overall speed" }), "512");
    await user.selectOptions(dialog.getByRole("combobox", { name: "Unit" }), "kb");
    fake.next("speedLimitSet", () => Promise.reject(callError("uncertain")));
    await user.click(dialog.getByRole("button", { name: "Save speed limit" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not confirm/i);
    // The service did save it; Reload shows its value, not the draft.
    fake.speedLimit = { global_bytes_per_second: "512000" };
    await user.click(dialog.getByRole("button", { name: "Reload speed limit" }));
    await waitFor(() =>
      expect(dialog.getByRole("textbox", { name: "Overall speed" })).toHaveValue("512"),
    );
    expect(dialog.getByRole("combobox", { name: "Unit" })).toHaveValue("kb");
  });
});

describe("a download's own speed limit (CORE-06)", () => {
  it("sets and removes the limit of the selected unfinished download from its details", async () => {
    const fake = supporting();
    fake.items = [snapshot({ job_id: "job_speed", display_name: "big.iso", attempt_id: "att_x" })];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("row", { name: /big\.iso/ }));
    expect(
      await screen.findByText("No limit of its own: only the overall limit applies."),
    ).toBeVisible();
    await user.type(screen.getByRole("textbox", { name: "Download speed" }), "256");
    await user.click(screen.getByRole("button", { name: "Set limit" }));
    expect(await screen.findByText("This download's own limit: 256 KB/s")).toBeVisible();
    expect(
      fake.calls.filter((c) => c.method === "downloadSpeedLimitSet").map((c) => c.args),
    ).toEqual([["job_speed", "256000"]]);
    await user.click(screen.getByRole("button", { name: "Remove limit" }));
    expect(
      await screen.findByText("No limit of its own: only the overall limit applies."),
    ).toBeVisible();
    await act(async () => {});
    expect(fake.items[0]?.speed_limit_bytes_per_second).toBeUndefined();
  });

  it("offers no control on a finished download", async () => {
    const fake = supporting();
    fake.items = [snapshot({ job_id: "job_done", display_name: "done.bin", phase: "completed" })];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("row", { name: /done\.bin/ }));
    expect(screen.queryByRole("textbox", { name: "Download speed" })).toBeNull();
  });

  it("has every speed-limit string in English and Bangla", () => {
    const keys = Object.keys(en).filter((k) => k.startsWith("speed."));
    expect(keys.length).toBeGreaterThan(20);
    for (const k of keys) expect(bn[k as keyof typeof bn], k).toBeTruthy();
    expect(bn["speed.heading"]).toBe("গতির সীমা");
  });
});
