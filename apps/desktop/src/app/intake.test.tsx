import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell } from "../test/fake-service";

function renderApp(client = new FakeServiceClient(), shell = new FakeShell()) {
  const view = render(
    <PreferencesProvider>
      <ServiceProvider client={client} shell={shell}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { ...view, client, shell };
}

function drop(target: Element, data: Record<string, string>, files: File[] = []) {
  const dataTransfer = {
    types: [...Object.keys(data), ...(files.length ? ["Files"] : [])],
    getData: (t: string) => data[t] ?? "",
    files,
    dropEffect: "none",
  };
  fireEvent.dragOver(target, { dataTransfer });
  fireEvent.drop(target, { dataTransfer });
}

describe("F03 drag and drop", () => {
  it("one dropped link opens Add URL filled in; nothing is added until the user chooses Add", async () => {
    const { container, client } = renderApp();
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    drop(container.querySelector(".app-window") as Element, {
      "text/uri-list": "https://example.org/disk.iso",
    });
    const dialog = await screen.findByRole("dialog", { name: "Add new download" });
    expect(within(dialog).getByRole("textbox", { name: /address/i })).toHaveValue(
      "https://example.org/disk.iso",
    );
    expect(client.count("add")).toBe(0);
  });

  it("several links open the batch dialog; an unsupported drop offers paste or import", async () => {
    const user = userEvent.setup();
    const { container, client } = renderApp();
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    const win = container.querySelector(".app-window") as Element;
    drop(win, { "text/plain": "https://example.org/a.zip https://example.org/b.zip" });
    const batch = await screen.findByRole("dialog", { name: "Add batch download" });
    expect(within(batch).getByText(/2 links to add/)).toBeVisible();
    await user.keyboard("{Escape}");
    drop(win, {}, [new File(["x"], "photo.png", { type: "image/png" })]);
    expect(await screen.findByText("Nothing to download in what was dropped")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Paste links…" }));
    expect(await screen.findByRole("dialog", { name: "Add batch download" })).toBeVisible();
    expect(client.count("add")).toBe(0);
  });
});

describe("F02 clipboard monitor", () => {
  it("is off by default, offers only new file links once on, and never adds by itself", async () => {
    const user = userEvent.setup();
    const shell = new FakeShell();
    shell.clipboard = "https://example.org/already-there.zip";
    let reads = 0;
    const read = shell.clipboardText.bind(shell);
    shell.clipboardText = async () => {
      reads += 1;
      return read();
    };
    const { client } = renderApp(new FakeServiceClient(), shell);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    await new Promise((r) => setTimeout(r, 50));
    expect(reads).toBe(0);

    await user.click(within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" }));
    const options = screen.getByRole("dialog", { name: "Options" });
    const box = within(options).getByRole("checkbox", { name: "Offer links I copy" });
    expect(box).not.toBeChecked();
    await user.click(box);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(reads).toBeGreaterThan(0));
    // What was already copied is not offered.
    expect(screen.queryByText("Download the link you copied?")).toBeNull();

    shell.clipboard = "see https://files.example.org/new-tool.exe";
    expect(
      await screen.findByText("Download the link you copied?", undefined, { timeout: 5000 }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Download…" }));
    const dialog = await screen.findByRole("dialog", { name: "Add new download" });
    expect(within(dialog).getByRole("textbox", { name: /address/i })).toHaveValue(
      "https://files.example.org/new-tool.exe",
    );
    expect(client.count("add")).toBe(0);
    await user.keyboard("{Escape}");

    shell.clipboard = "https://files.example.org/other.msi";
    expect(
      await screen.findByText("Download the link you copied?", undefined, { timeout: 5000 }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Ignore files.example.org" }));
    shell.clipboard = "https://files.example.org/third.zip";
    await new Promise((r) => setTimeout(r, 2500));
    expect(screen.queryByText("Download the link you copied?")).toBeNull();
  }, 20_000);
});
