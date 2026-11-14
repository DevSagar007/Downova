import { describe, expect, it } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FoundationServiceClient, type ServiceClient } from "../services/service-client";
import { FakeServiceClient, snapshot } from "../test/fake-service";

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

  it("keeps service commands focusable but unavailable, with a reason", async () => {
    renderApp();
    await screen.findByText("Download service not connected");
    const toolbar = screen.getByRole("toolbar");
    const add = within(toolbar).getByRole("button", { name: "Add URL" });
    expect(add).toHaveAttribute("aria-disabled", "true");
    expect(add).toHaveAccessibleDescription(/background download service/i);
    expect(within(toolbar).getByRole("button", { name: "Options" })).not.toHaveAttribute(
      "aria-disabled",
    );
  });

  it("shows a connected service truthfully and wires only implemented commands", async () => {
    renderApp(new FakeServiceClient());
    expect(await screen.findByText("Connected to download service")).toBeVisible();
    const toolbar = screen.getByRole("toolbar");
    expect(within(toolbar).getByRole("button", { name: "Add URL" })).not.toHaveAttribute(
      "aria-disabled",
    );
    const remove = within(toolbar).getByRole("button", { name: "Delete" });
    expect(remove).toHaveAttribute("aria-disabled", "true");
    expect(remove).toHaveAccessibleDescription(
      /deleting downloaded files is not available.*keeps the file/i,
    );
    // IDM parity (2026-11-14): no toolbar command waits for "a later milestone" any more.
    for (const name of ["Stop all", "Delete completed", "Scheduler"]) {
      expect(within(toolbar).getByRole("button", { name })).not.toHaveAccessibleDescription(
        /later milestone/i,
      );
    }
  });

  it("Stop all pauses every download that can be paused and nothing else", async () => {
    const user = userEvent.setup();
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({ job_id: "job_a", display_name: "a.zip" }),
      snapshot({ job_id: "job_b", display_name: "b.zip", phase: "queued" }),
      snapshot({
        job_id: "job_c",
        display_name: "c.zip",
        desired_state: "paused",
        phase: "queued",
      }),
      snapshot({ job_id: "job_d", display_name: "d.zip", phase: "completed" }),
    ];
    renderApp(fake);
    expect(await screen.findByText("a.zip")).toBeVisible();
    const stopAll = within(screen.getByRole("toolbar")).getByRole("button", { name: "Stop all" });
    expect(stopAll).not.toHaveAttribute("aria-disabled");
    await user.click(stopAll);
    await waitFor(() => expect(fake.count("pause")).toBe(2));
    const paused = fake.calls.filter((c) => c.method === "pause").map((c) => c.args[0]);
    expect(paused.sort()).toEqual(["job_a", "job_b"]);
  });

  it("File → Properties shows what the service reports about the selected download", async () => {
    const user = userEvent.setup();
    const fake = new FakeServiceClient();
    fake.items = [
      snapshot({
        job_id: "job_p",
        display_name: "report.pdf",
        source_display: "https://files.example.org/report.pdf?…",
        total_bytes: "2048",
        downloaded_bytes: "1024",
        priority: "high",
      }),
    ];
    renderApp(fake);
    expect(await screen.findByText("report.pdf")).toBeVisible();
    await user.click(screen.getByRole("menuitem", { name: "File" }));
    expect(screen.getByRole("menuitem", { name: "Properties" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    await user.click(document.querySelector('[data-job-id="job_p"]') as HTMLElement);
    await user.click(screen.getByRole("menuitem", { name: "File" }));
    await user.click(screen.getByRole("menuitem", { name: "Properties" }));
    const dialog = await screen.findByRole("dialog", { name: "Properties" });
    expect(within(dialog).getByText("https://files.example.org/report.pdf?…")).toBeVisible();
    expect(within(dialog).getByText("2.0 KB")).toBeVisible();
    expect(within(dialog).getByText("High")).toBeVisible();
    expect(within(dialog).getByText("Documents (by file type)")).toBeVisible();
    expect(within(dialog).getByText("Main download queue")).toBeVisible();
  });

  it("Scheduler opens the queues, Delete completed asks first, Speed limiter opens Options", async () => {
    const user = userEvent.setup();
    const fake = new FakeServiceClient();
    fake.items = [snapshot({ job_id: "job_d", display_name: "d.zip", phase: "completed" })];
    renderApp(fake);
    expect(await screen.findByText("d.zip")).toBeVisible();
    const toolbar = screen.getByRole("toolbar");
    await user.click(within(toolbar).getByRole("button", { name: "Scheduler" }));
    expect(await screen.findByRole("dialog", { name: /queues/i })).toBeVisible();
    await user.keyboard("{Escape}");
    await user.click(within(toolbar).getByRole("button", { name: "Delete completed" }));
    expect(await screen.findByRole("dialog", { name: "Clear finished downloads?" })).toBeVisible();
    expect(fake.count("remove")).toBe(0);
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("menuitem", { name: "Downloads" }));
    await user.click(screen.getByRole("menuitem", { name: "Speed limiter…" }));
    expect(await screen.findByRole("dialog", { name: "Options" })).toBeVisible();
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
