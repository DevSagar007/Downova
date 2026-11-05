// CORE-09: Options → Notifications. The service decides and shows notifications; the window only
// changes the setting and keeps the service's language in step.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell } from "../test/fake-service";

function setup(fake = new FakeServiceClient()) {
  fake.streaming = true;
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()} reconnectDelaysMs={[20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { fake, user: userEvent.setup() };
}

async function openOptions(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  return screen.getByRole("dialog", { name: "Options" });
}

describe("notification settings (CORE-09)", () => {
  it("is off by default and turning it on is one explicit request in the window's language", async () => {
    const { fake, user } = setup();
    const dialog = await openOptions(user);
    const box = await within(dialog).findByRole("checkbox", {
      name: "Notify me when a download completes or fails",
    });
    expect(box).not.toBeChecked();
    expect(box).toHaveAccessibleDescription(/also while this window is hidden or closed/);
    expect(box).toHaveAccessibleDescription(/never open a downloaded file/);
    expect(fake.count("notificationsSet")).toBe(0);
    await user.click(box);
    await waitFor(() => expect(box).toBeChecked());
    expect(fake.calls.filter((c) => c.method === "notificationsSet").map((c) => c.args)).toEqual([
      ["true", "en"],
    ]);
  });

  it("an unavailable system is explained and cannot be turned on", async () => {
    const fake = new FakeServiceClient();
    fake.notifications = { ...fake.notifications, availability: "unavailable" };
    const { user } = setup(fake);
    const dialog = await openOptions(user);
    const box = await within(dialog).findByRole("checkbox", {
      name: "Notify me when a download completes or fails",
    });
    expect(box).toHaveAccessibleDescription(/cannot show notifications/);
    await user.click(box);
    expect(box).not.toBeChecked();
    expect(fake.count("notificationsSet")).toBe(0);
  });

  it("a blocked system is reported without asking again", async () => {
    const fake = new FakeServiceClient();
    fake.notifications = { ...fake.notifications, enabled: true, last_delivery_failed: true };
    const { user } = setup(fake);
    const dialog = await openOptions(user);
    expect(await within(dialog).findByText(/refused by the system/)).toBeVisible();
    expect(fake.count("notificationsSet")).toBe(0);
  });

  it("switching the window to Bangla tells the service, only while notifications are on", async () => {
    const fake = new FakeServiceClient();
    fake.notifications = { ...fake.notifications, enabled: true };
    const { user } = setup(fake);
    await screen.findByRole("button", { name: "Options" });
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    await user.click(await screen.findByRole("menuitemradio", { name: /বাংলা/ }));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "notificationsSet").map((c) => c.args)).toEqual([
        ["true", "bn"],
      ]),
    );
  });
});
