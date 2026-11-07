import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import type { BrowserConnection, BrowserRepairOutcome } from "../services/service-client";
import { FakeServiceClient, FakeShell } from "../test/fake-service";
import { bn, en } from "../i18n/messages";

const native = (browser: string, registration: string): BrowserConnection => ({
  browser,
  registration,
  packages: ["native"],
  support: [{ package: "native", support: "supported", reason: "tested_this_os" }],
});

async function open(fake = new FakeServiceClient()) {
  fake.streaming = true;
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("menuitem", { name: "Help" }));
  await user.click(screen.getByRole("menuitem", { name: "Browser connections…" }));
  const dialog = within(screen.getByRole("dialog", { name: "Browser connections" }));
  return { fake, user, dialog };
}

function item(browser: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(`[data-browser="${browser}"]`);
  if (!el) throw new Error(`no ${browser}`);
  return el;
}

describe("browser connections (BRW-07/09)", () => {
  it("shows each found browser's package, connection and support; hides browsers not found", async () => {
    const fake = new FakeServiceClient();
    fake.browsers = [
      native("chrome", "ours"),
      native("firefox", "absent"),
      {
        browser: "chromium",
        registration: "absent",
        packages: [],
        support: [{ package: "unknown", support: "untested", reason: "not_detected" }],
      },
      {
        browser: "edge",
        registration: "ours",
        packages: ["flatpak"],
        support: [{ package: "flatpak", support: "unsupported", reason: "flatpak_sandbox" }],
      },
    ];
    const { dialog } = await open(fake);
    expect(await dialog.findByRole("heading", { name: "Google Chrome" })).toBeVisible();
    expect(within(item("chrome")).getByText("Connected to this Downova")).toBeVisible();
    expect(within(item("firefox")).getByText("Not connected")).toBeVisible();
    expect(
      within(item("firefox")).getByText(/Downloads with your sign-in, reconnecting them/),
    ).toBeVisible();
    expect(within(item("edge")).getByText("Flatpak package")).toBeVisible();
    expect(within(item("edge")).getByText(/Support: Not supported/)).toBeVisible();
    expect(within(item("edge")).getByText(/cannot start Downova's helper/)).toBeVisible();
    expect(document.querySelector('[data-browser="chromium"]')).toBeNull();
    expect(dialog.getByText("Other browsers were not found where Downova looks.")).toBeVisible();
    expect(document.body.textContent).not.toMatch(/[A-Z]:\\|\/home\/|\.json/);
  });

  it("repairs once however fast the clicks come, and shows the new connection", async () => {
    const fake = new FakeServiceClient();
    fake.browsers = [native("firefox", "stale")];
    const { dialog } = await open(fake);
    const repair = await dialog.findByRole("button", { name: "Repair the connection for Firefox" });
    act(() => {
      repair.click();
      repair.click();
    });
    expect(
      await within(item("firefox")).findByText("Repaired. If the browser was open, restart it."),
    ).toBeVisible();
    expect(fake.count("browserRepair")).toBe(1);
    expect(fake.calls.find((c) => c.method === "browserRepair")?.args).toEqual(["firefox"]);
    expect(within(item("firefox")).getByText("Connected to this Downova")).toBeVisible();
  });

  it("explains a refusal and never claims another installation's entry", async () => {
    const fake = new FakeServiceClient();
    fake.browsers = [native("chrome", "other_installation")];
    const { user, dialog } = await open(fake);
    await user.click(
      await dialog.findByRole("button", { name: "Repair the connection for Google Chrome" }),
    );
    expect(
      await within(item("chrome")).findByText(/another Downova installation uses this connection/),
    ).toBeVisible();
    expect(
      within(item("chrome")).getByText("Connected to another Downova installation"),
    ).toBeVisible();
  });

  it("an answer from before Check again is never shown", async () => {
    const fake = new FakeServiceClient();
    fake.browsers = [native("firefox", "absent")];
    let release: (o: BrowserRepairOutcome) => void = () => {};
    fake.next("browserRepair", () => new Promise((r) => (release = r)));
    const { user, dialog } = await open(fake);
    await user.click(
      await dialog.findByRole("button", { name: "Repair the connection for Firefox" }),
    );
    await user.click(dialog.getByRole("button", { name: "Check again" }));
    await dialog.findByRole("heading", { name: "Firefox" });
    await act(async () => release({ outcome: "repaired", connection: native("firefox", "ours") }));
    expect(screen.queryByText(/Repaired\./)).toBeNull();
    expect(within(item("firefox")).getByText("Not connected")).toBeVisible();
  });

  it("says plainly when the helper is not available", async () => {
    const fake = new FakeServiceClient();
    fake.next("browserConnections", async () => {
      throw new Error("no helper");
    });
    const { dialog } = await open(fake);
    expect(await dialog.findByText(/browser helper is not available here/)).toBeVisible();
  });

  it("has every browser-connection string in Bangla", () => {
    const keys = Object.keys(en).filter(
      (k) =>
        (k.startsWith("browsers.") && !k.startsWith("browsers.name.")) ||
        k === "menu.help.browsers",
    );
    expect(keys.length).toBeGreaterThanOrEqual(40);
    for (const k of keys) {
      expect(bn[k as keyof typeof bn], k).toBeTruthy();
      expect(bn[k as keyof typeof bn], k).not.toBe(en[k as keyof typeof en]);
    }
  });
});
