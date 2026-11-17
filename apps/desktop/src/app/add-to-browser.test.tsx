// ADR 0041: "Add Downova to Chrome/Edge" without a store listing, and the one-time offer.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import type { BrowserConnection } from "../services/service-client";
import { FakeServiceClient, FakeShell } from "../test/fake-service";
import { bn, en } from "../i18n/messages";

const native = (browser: string): BrowserConnection => ({
  browser,
  registration: "ours",
  packages: ["native"],
  support: [{ package: "native", support: "supported", reason: "tested_this_os" }],
});

function renderApp(shell: FakeShell, fake = new FakeServiceClient()) {
  fake.streaming = true;
  fake.browsers = [native("chrome"), native("edge"), native("firefox")];
  return render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={shell}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

afterEach(() => localStorage.clear());

describe("add to browser (ADR 0041)", () => {
  it("offers once to connect a browser, and Show me how opens the guide", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    shell.addInfo = { extension_bundled: true, browsers_found: ["chrome"] };
    const first = renderApp(shell);
    expect(await screen.findByText("Use Downova from your browser")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Show me how" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Browser connections" }));
    expect(
      await dialog.findByRole("heading", { name: "Add Downova to Google Chrome" }),
    ).toBeVisible();
    first.unmount();
    // Answered once: it does not come back.
    renderApp(shell);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    expect(screen.queryByText("Use Downova from your browser")).toBeNull();
  });

  it("is not offered without a bundled extension or without a guided browser", async () => {
    for (const info of [
      { extension_bundled: false, browsers_found: ["chrome"] },
      { extension_bundled: true, browsers_found: [] },
    ]) {
      const shell = new FakeShell();
      shell.addInfo = info;
      const r = renderApp(shell);
      expect(await screen.findByText("Connected to download service")).toBeVisible();
      expect(screen.queryByText("Use Downova from your browser")).toBeNull();
      r.unmount();
    }
  });

  it("guides Chrome and Edge: opens the browser, copies the page address, shows the folder", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    shell.addInfo = { extension_bundled: true, browsers_found: ["chrome"] };
    renderApp(shell);
    await user.click(await screen.findByRole("button", { name: "Not now" }));
    await user.click(screen.getByRole("menuitem", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "Browser connections…" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Browser connections" }));
    const chrome = within(
      await dialog.findByRole("region", { name: "Add Downova to Google Chrome" }),
    );
    expect(chrome.getByText("chrome://extensions")).toBeVisible();
    await user.click(chrome.getByRole("button", { name: "Open Google Chrome" }));
    await user.click(chrome.getByRole("button", { name: "Copy chrome://extensions" }));
    expect(
      await chrome.findByText("Copied chrome://extensions. Paste it into the address bar."),
    ).toBeVisible();
    await user.click(chrome.getByRole("button", { name: "Show the extension folder" }));
    expect(shell.browserActions).toEqual(["open chrome", "copy chrome", "folder"]);
    // Edge is not installed here: no Open button, a note instead; copying still works.
    const edge = within(dialog.getByRole("region", { name: "Add Downova to Microsoft Edge" }));
    expect(edge.queryByRole("button", { name: "Open Microsoft Edge" })).toBeNull();
    expect(edge.getByText("Microsoft Edge was not found on this computer.")).toBeVisible();
    expect(edge.getByText("edge://extensions")).toBeVisible();
    // Firefox gets no guide: only signed add-ons install there.
    expect(
      dialog.getByText(
        "Firefox installs only add-ons signed by Mozilla. A signed Downova add-on is not available yet.",
      ),
    ).toBeVisible();
  });

  it("says why there is nothing to add in a development build", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    renderApp(shell);
    await user.click(await screen.findByRole("menuitem", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "Browser connections…" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Browser connections" }));
    const chrome = within(
      await dialog.findByRole("region", { name: "Add Downova to Google Chrome" }),
    );
    expect(chrome.getByText(/no extension folder/)).toBeVisible();
    expect(chrome.queryByRole("button")).toBeNull();
  });

  it("has every string in Bangla", () => {
    const keys = Object.keys(en).filter(
      (k) => k.startsWith("browsers.add.") || k.startsWith("connectBrowser."),
    );
    expect(keys.length).toBeGreaterThanOrEqual(20);
    for (const k of keys) {
      expect(bn[k as keyof typeof bn], k).toBeTruthy();
      expect(bn[k as keyof typeof bn], k).not.toBe(en[k as keyof typeof en]);
    }
  });
});
