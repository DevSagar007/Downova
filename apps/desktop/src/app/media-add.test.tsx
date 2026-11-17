import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, HELLO } from "../test/fake-service";

function renderLive(fake: FakeServiceClient) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={fake} pollIntervalMs={20} reconnectDelaysMs={[20, 20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

const command = (name: string) => within(screen.getByRole("toolbar")).getByRole("button", { name });

const WITH_MEDIA = {
  ...HELLO,
  media: { id: "yt-dlp", version: "2026.10.26", capabilities: [] },
};

async function openAdd(fake: FakeServiceClient) {
  const user = userEvent.setup();
  renderLive(fake);
  await waitFor(() => expect(command("Add URL")).not.toHaveAttribute("aria-disabled"));
  await user.click(command("Add URL"));
  return { user, dialog: screen.getByRole("dialog", { name: "Add new download" }) };
}

describe("video and audio from a web page (ADR 0043)", () => {
  it("sends the media engine only when chosen, so ordinary adds are unchanged", async () => {
    const fake = new FakeServiceClient();
    fake.connection = { status: "connected", hello: WITH_MEDIA };
    const { user, dialog } = await openAdd(fake);
    const media = within(dialog).getByRole("checkbox", { name: "Video or audio on a web page" });
    expect(media).toBeEnabled();
    expect(media).not.toBeChecked();
    await user.type(within(dialog).getByLabelText("Address (URL)"), "https://h/a.zip");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fake.count("add")).toBe(1));
    expect(fake.lastAddOptions?.engine).toBeUndefined();

    await user.click(command("Add URL"));
    const again = screen.getByRole("dialog", { name: "Add new download" });
    await user.type(within(again).getByLabelText("Address (URL)"), "https://h/watch?v=1");
    // Keyboard: Space toggles the labelled checkbox.
    within(again).getByRole("checkbox", { name: "Video or audio on a web page" }).focus();
    await user.keyboard(" ");
    await user.click(within(again).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fake.count("add")).toBe(2));
    expect(fake.lastAddOptions?.engine).toBe("media");
  });

  it("is unavailable, with the reason, when the service has no media engine", async () => {
    const fake = new FakeServiceClient();
    const { dialog } = await openAdd(fake);
    const media = within(dialog).getByRole("checkbox", { name: "Video or audio on a web page" });
    expect(media).toBeDisabled();
    expect(media).toHaveAccessibleDescription(
      /need the media engine \(yt-dlp\), which is not installed/,
    );
  });

  it("explains the choice in Bangla too", async () => {
    const fake = new FakeServiceClient();
    fake.connection = { status: "connected", hello: WITH_MEDIA };
    const user = userEvent.setup();
    renderLive(fake);
    await waitFor(() => expect(command("Add URL")).not.toHaveAttribute("aria-disabled"));
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    await user.click(await screen.findByRole("menuitemradio", { name: /বাংলা/ }));
    await user.click(await screen.findByRole("button", { name: "URL যোগ" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("checkbox", { name: "ওয়েব পাতার ভিডিও বা অডিও" }),
    ).toHaveAccessibleDescription(/কপি-সুরক্ষিত \(DRM\) কনটেন্ট নামানো হয় না/);
  });
});
