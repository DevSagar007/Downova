import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../app/App";
import { PreferencesProvider } from "../../app/preferences";
import { ServiceProvider } from "../../services/ServiceProvider";
import { FakeServiceClient, FakeShell, callError } from "../../test/fake-service";

function renderApp(client: FakeServiceClient, shell = new FakeShell()) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={client} shell={shell}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

describe("Add batch download / Import download list (IDM parity)", () => {
  it("adds every link from the clipboard once, reports a duplicate, and repeats nothing", async () => {
    const user = userEvent.setup();
    const fake = new FakeServiceClient();
    fake.next("add", async () => {
      throw callError("rejected", "DUPLICATE_DOWNLOAD", "none");
    });
    const shell = new FakeShell();
    shell.clipboard =
      "https://example.org/dup.zip\nnot a link\nhttps://example.org/a.iso https://example.org/b.pdf\n";
    renderApp(fake, shell);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    await user.click(screen.getByRole("menuitem", { name: "Tasks" }));
    await user.click(screen.getByRole("menuitem", { name: "Add batch download from clipboard" }));
    const dialog = await screen.findByRole("dialog", { name: "Add batch download" });
    expect(within(dialog).getByText(/3 links to add, 3 other entries ignored/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Add 3 downloads" }));
    expect(
      await within(dialog).findByText("2 added, 1 already in the list, 0 not added."),
    ).toBeVisible();
    const sent = fake.calls.filter((c) => c.method === "add");
    expect(sent.map((c) => c.args[0])).toEqual([
      "https://example.org/dup.zip",
      "https://example.org/a.iso",
      "https://example.org/b.pdf",
    ]);
    expect(new Set(sent.map((c) => c.args[1])).size).toBe(3);
    // Nothing is left to add: the button says why instead of sending again.
    const again = within(dialog).getByRole("button", { name: "Add 0 downloads" });
    expect(again).toHaveAttribute("aria-disabled", "true");
    await user.click(again);
    expect(fake.count("add")).toBe(3);
  });

  it("opens empty with the reason when the clipboard cannot be read", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    shell.clipboardFails = true;
    renderApp(new FakeServiceClient(), shell);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    await user.click(screen.getByRole("menuitem", { name: "Tasks" }));
    await user.click(screen.getByRole("menuitem", { name: "Add batch download from clipboard" }));
    const dialog = await screen.findByRole("dialog", { name: "Add batch download" });
    expect(within(dialog).getByText(/clipboard could not be read/i)).toBeVisible();
    expect(within(dialog).getByRole("textbox")).toHaveValue("");
  });

  it("imports a list file into the same dialog", async () => {
    const fake = new FakeServiceClient();
    const { container } = renderApp(fake);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(
      ["https://example.org/x.bin\r\nhttps://example.org/y.bin\r\n"],
      "list.txt",
      {
        type: "text/plain",
      },
    );
    fireEvent.change(input, { target: { files: [file] } });
    const dialog = await screen.findByRole("dialog", { name: "Add batch download" });
    await waitFor(() =>
      expect(within(dialog).getByText(/2 links to add, 0 other entries ignored/)).toBeVisible(),
    );
  });
});
