import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FoundationServiceClient, type ServiceClient } from "../services/service-client";

function renderApp(client: ServiceClient = new FoundationServiceClient()) {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={client}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

describe("classic download-manager window", () => {
  it("shows menu bar, toolbar, categories, column headers and status bar", async () => {
    renderApp();
    expect(screen.getByRole("menubar", { name: "Application menu" })).toBeVisible();
    expect(screen.getByRole("toolbar", { name: "Download commands" })).toBeVisible();
    expect(screen.getByRole("navigation", { name: "Categories" })).toBeVisible();
    for (const name of [
      "File name",
      "Size",
      "Status",
      "Time left",
      "Transfer rate",
      "Last try",
      "Description",
    ]) {
      expect(screen.getByRole("columnheader", { name })).toBeVisible();
    }
    expect(await screen.findByText("Download service not connected")).toBeVisible();
    expect(screen.getByText("No downloads in this category.")).toBeVisible();
  });

  it("keeps service commands focusable but unavailable, with a reason", () => {
    renderApp();
    const toolbar = screen.getByRole("toolbar");
    const add = within(toolbar).getByRole("button", { name: "Add URL" });
    expect(add).toHaveAttribute("aria-disabled", "true");
    expect(add).toHaveAccessibleDescription(/background download service/i);
    expect(within(toolbar).getByRole("button", { name: "Options" })).not.toHaveAttribute(
      "aria-disabled",
    );
  });

  it("shows a connected service truthfully while commands stay unavailable", async () => {
    const connected: ServiceClient = {
      connect: async () => ({
        status: "connected",
        hello: {
          api_version: 1,
          service_version: "0.1.0",
          service_instance_id: "svc_test",
          backend: { id: "http", version: "0.1.0", capabilities: [] },
        } as never,
      }),
    };
    renderApp(connected);
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    const add = within(screen.getByRole("toolbar")).getByRole("button", { name: "Add URL" });
    expect(add).toHaveAttribute("aria-disabled", "true");
    expect(add).toHaveAccessibleDescription(/connected to it in a later milestone/i);
  });

  it("moves through toolbar commands with arrow keys", async () => {
    const user = userEvent.setup();
    renderApp();
    const toolbar = screen.getByRole("toolbar");
    within(toolbar).getByRole("button", { name: "Add URL" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(within(toolbar).getByRole("button", { name: "Resume" })).toHaveFocus();
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(within(toolbar).getByRole("button", { name: "Stop queue" })).toHaveFocus();
  });

  it("selects and collapses categories", async () => {
    const user = userEvent.setup();
    renderApp();
    const nav = screen.getByRole("navigation", { name: "Categories" });
    expect(within(nav).getByRole("button", { name: "All downloads" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    await user.click(within(nav).getByRole("button", { name: "Video" }));
    expect(within(nav).getByRole("button", { name: "Video" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByRole("table", { name: "Downloads in Video" })).toBeVisible();
    await user.click(within(nav).getByRole("button", { name: "Collapse: All downloads" }));
    expect(within(nav).queryByRole("button", { name: "Video" })).not.toBeInTheDocument();
  });

  it("opens Options from the toolbar, switches to Bangla, and returns focus on Escape", async () => {
    const user = userEvent.setup();
    renderApp();
    const options = within(screen.getByRole("toolbar")).getByRole("button", { name: "Options" });
    await user.click(options);
    const dialog = screen.getByRole("dialog", { name: "Options" });
    await user.selectOptions(within(dialog).getByLabelText("Language"), "bn");
    expect(document.documentElement.lang).toBe("bn");
    expect(screen.getByRole("dialog", { name: "অপশন" })).toBeVisible();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(within(screen.getByRole("toolbar")).getByRole("button", { name: "অপশন" })).toHaveFocus();
  });

  it("operates the menu bar from the keyboard and changes theme", async () => {
    const user = userEvent.setup();
    renderApp();
    const tasks = screen.getByRole("menuitem", { name: "Tasks" });
    tasks.focus();
    await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
    const view = screen.getByRole("menuitem", { name: "View" });
    expect(view).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    const menu = screen.getByRole("menu", { name: "View" });
    await user.click(within(menu).getByRole("menuitemradio", { name: "Theme: dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("hides the category panel from the View menu", async () => {
    const user = userEvent.setup();
    renderApp();
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    await user.click(screen.getByRole("menuitemcheckbox", { name: "Show categories" }));
    expect(screen.queryByRole("navigation", { name: "Categories" })).not.toBeInTheDocument();
  });
});

describe("categories panel close button", () => {
  it("hides the panel and View menu shows it again", async () => {
    const user = userEvent.setup();
    renderApp();
    await user.click(screen.getByRole("button", { name: "Hide categories" }));
    expect(screen.queryByRole("navigation", { name: "Categories" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    const toggle = screen.getByRole("menuitemcheckbox", { name: "Show categories" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    expect(screen.getByRole("navigation", { name: "Categories" })).toBeVisible();
  });
});
