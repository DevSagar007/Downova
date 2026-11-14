import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient } from "../test/fake-service";

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

describe("anonymous FTP links (F05)", () => {
  it("adds an ftp:// link, saying first that FTP is not encrypted", async () => {
    const fake = new FakeServiceClient();
    const user = userEvent.setup();
    renderLive(fake);
    await waitFor(() => expect(command("Add URL")).not.toHaveAttribute("aria-disabled"));
    await user.click(command("Add URL"));
    const dialog = screen.getByRole("dialog", { name: "Add new download" });
    const field = within(dialog).getByLabelText("Address (URL)");
    expect(within(dialog).queryByRole("note")).toBeNull();
    await user.type(field, "ftp://ftp.example.org/pub/image.iso");
    expect(within(dialog).getByRole("note")).toHaveTextContent(/FTP is not encrypted/);
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "add").map((c) => c.args[0])).toEqual([
        "ftp://ftp.example.org/pub/image.iso",
      ]),
    );
  });

  it("refuses an FTP link with a user name or password without sending it", async () => {
    const fake = new FakeServiceClient();
    const user = userEvent.setup();
    renderLive(fake);
    await waitFor(() => expect(command("Add URL")).not.toHaveAttribute("aria-disabled"));
    await user.click(command("Add URL"));
    const dialog = screen.getByRole("dialog", { name: "Add new download" });
    await user.type(
      within(dialog).getByLabelText("Address (URL)"),
      "ftp://user:pw@ftp.example.org/a.iso",
    );
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(/user name or password/);
    expect(fake.calls.some((c) => c.method === "add")).toBe(false);
  });
});
