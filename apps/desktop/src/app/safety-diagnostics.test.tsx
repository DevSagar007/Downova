import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { DiagnosticsPreview, DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, callError, snapshot } from "../test/fake-service";
import { bn, en } from "../i18n/messages";

function setup(fake = new FakeServiceClient()) {
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

async function openDiagnostics(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("menuitem", { name: "Help" }));
  await user.click(screen.getByRole("menuitem", { name: "Diagnostics…" }));
  return within(screen.getByRole("dialog", { name: "Diagnostics" }));
}

describe("diagnostics preview and export (E05)", () => {
  it("previews before saving, keeps the crash section opt-in, and saves exactly the shown snapshot once", async () => {
    const { fake, user } = setup();
    const d = await openDiagnostics(user);
    const region = await d.findByRole("textbox", { name: "Diagnostics preview" });
    expect((region as HTMLTextAreaElement).value).toContain('"schema_version": 1');
    expect((region as HTMLTextAreaElement).value).not.toContain("crash");
    expect(fake.calls.find((c) => c.method === "diagnosticsPreview")?.args).toEqual(["false"]);
    await user.click(
      d.getByRole("checkbox", {
        name: "Include the last startup's recovery summary (crash-related)",
      }),
    );
    await waitFor(() => expect((region as HTMLTextAreaElement).value).toContain('"crash"'));
    expect(fake.count("diagnosticsPreview")).toBe(2);
    const save = d.getByRole("button", { name: "Save…" });
    act(() => {
      save.click();
      save.click();
    });
    expect(
      await d.findByText("Saved as downova-diagnostics.json (42 bytes). Nothing was sent."),
    ).toBeVisible();
    const exports = fake.calls.filter((c) => c.method === "diagnosticsExport");
    expect(exports).toHaveLength(1);
    expect(exports[0]?.args).toEqual(["diag_0000000000000002", "Save Downova diagnostics"]);
  });

  it("a cancelled save dialog writes nothing and a refused save is explained", async () => {
    const fake = new FakeServiceClient();
    fake.exportOutcome = { outcome: "cancelled" };
    const { user } = setup(fake);
    const d = await openDiagnostics(user);
    await d.findByRole("textbox", { name: "Diagnostics preview" });
    await user.click(d.getByRole("button", { name: "Save…" }));
    expect(await d.findByText("Not saved.")).toBeVisible();
    fake.next("diagnosticsExport", async () => {
      throw callError("rejected", "DISK_FULL", "free_disk_space");
    });
    await user.click(d.getByRole("button", { name: "Save…" }));
    expect(await d.findByText("The diagnostics were not saved.")).toBeVisible();
  });

  it("an older preview answering late never replaces the newer one", async () => {
    const fake = new FakeServiceClient();
    let release: (p: DiagnosticsPreview) => void = () => {};
    fake.next("diagnosticsPreview", () => new Promise((r) => (release = r)));
    const { user } = setup(fake);
    const d = await openDiagnostics(user);
    await user.click(
      d.getByRole("checkbox", {
        name: "Include the last startup's recovery summary (crash-related)",
      }),
    );
    const region = await d.findByRole("textbox", { name: "Diagnostics preview" });
    await waitFor(() => expect((region as HTMLTextAreaElement).value).toContain('"crash"'));
    await act(async () =>
      release({ snapshot_id: "diag_00000000000000ff", content: "STALE", size_bytes: "5" }),
    );
    expect((region as HTMLTextAreaElement).value).not.toContain("STALE");
  });
});

function job(scan: DownloadSnapshot["scan"], phase: DownloadSnapshot["phase"] = "completed") {
  return snapshot({
    job_id: "job_scan",
    display_name: "setup.exe",
    phase,
    ...(scan ? { scan } : {}),
    total_bytes: "10",
    downloaded_bytes: "10",
    ...(phase === "completed"
      ? { verification: { sha256: "a".repeat(64), source: "local_hash" as const } }
      : {}),
    ...(phase === "failed"
      ? { error: { kind: "SCAN_BLOCKED" as const, retryable: false, next_action: "none" as const } }
      : {}),
  });
}

async function details(scan: DownloadSnapshot["scan"], phase?: DownloadSnapshot["phase"]) {
  const fake = new FakeServiceClient();
  fake.items = [job(scan, phase)];
  const { user } = setup(fake);
  await user.click(await screen.findByRole("button", { name: "setup.exe" }));
  return user;
}

describe("attachment check states (F07, E06)", () => {
  it("describes a passed check as what Windows did, never as safe", async () => {
    await details({ status: "attachment_checked", origin_marking: "present" });
    expect(await screen.findByText("Checked by Windows Attachment Services")).toBeVisible();
    expect(screen.getByText(/This is not a malware verdict/)).toBeVisible();
    expect(screen.getByText(/not that it is safe/)).toBeVisible();
    // The only "safe" is the negation in the Mark of the Web line.
    expect(document.body.textContent).not.toMatch(
      /malware-free|verified safe|(?<!not that it )is safe\b/i,
    );
    expect(screen.getByRole("row", { name: /setup\.exe/ })).toHaveTextContent("Complete");
  });

  it("shows scanning, blocked and unavailable in plain words", async () => {
    await details({ status: "scanning" }, "scanning");
    expect(await screen.findByText("Checking with Windows security")).toBeVisible();
  });

  it("a blocked file is a failure with its reason", async () => {
    await details({ status: "blocked", reason: "file_removed" }, "failed");
    expect(await screen.findByText("Blocked by Windows security")).toBeVisible();
    expect(screen.getByText(/removed during the check/)).toBeVisible();
    expect(screen.getByText("Downova did not keep or re-create the file.")).toBeVisible();
    expect(screen.getByRole("row", { name: /setup\.exe/ })).not.toHaveTextContent(/Complete/);
  });

  it("an unchecked completed download keeps a visible warning in the list and details", async () => {
    await details({
      status: "unavailable",
      reason: "service_error",
      origin_marking: "written_by_downova",
    });
    expect(screen.getByRole("row", { name: /setup\.exe/ })).toHaveTextContent(
      "Complete (not checked)",
    );
    expect(await screen.findByText("Not checked")).toBeVisible();
    expect(screen.getByText(/Be careful before opening it/)).toBeVisible();
  });

  it("macOS and Linux say no check ran, never that Windows did", async () => {
    await details({
      status: "skipped",
      reason: "platform_not_supported",
      origin_marking: "not_supported_on_platform",
    });
    expect(await screen.findByText("No malware check on this system")).toBeVisible();
    expect(document.body.textContent).not.toMatch(/Attachment Services (processed|checked)/);
  });

  it("a download finished before checks existed shows no check at all", async () => {
    await details(undefined);
    await screen.findByRole("row", { name: /setup\.exe/ });
    expect(screen.queryByText(/Windows Attachment Services|Not checked|malware check/)).toBeNull();
  });

  it("has every check and diagnostics string in Bangla", () => {
    const keys = Object.keys(en).filter(
      (k) => k.startsWith("scan.") || k.startsWith("diagnostics.") || k === "menu.help.diagnostics",
    );
    expect(keys.length).toBeGreaterThanOrEqual(40);
    for (const k of keys) {
      expect(bn[k as keyof typeof bn], k).toBeTruthy();
      expect(bn[k as keyof typeof bn], k).not.toBe(en[k as keyof typeof en]);
    }
  });
});
