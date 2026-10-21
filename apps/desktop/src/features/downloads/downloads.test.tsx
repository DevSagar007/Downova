import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DownloadSnapshot } from "@dm/contracts";
import { PreferencesProvider } from "../../app/preferences";
import { en } from "../../i18n/messages";
import { DownloadTable } from "./DownloadTable";
import { matchesCategory } from "./categories";
import { formatBytes, percentOf, statusText } from "./present";

const t = (key: keyof typeof en) => en[key];

function snapshot(overrides: Partial<DownloadSnapshot>): DownloadSnapshot {
  return {
    job_id: "job_1",
    attempt_id: null,
    display_name: "file.zip",
    source_display: "https://example.org/file.zip",
    desired_state: "running",
    phase: "transferring",
    blockers: [],
    downloaded_bytes: "0",
    total_bytes: null,
    error: null,
    ...overrides,
  };
}

describe("presentation helpers", () => {
  it("formats byte counts beyond 2^53 without precision loss", () => {
    expect(formatBytes("0")).toBe("0 B");
    expect(formatBytes("1536")).toBe("1.5 KB");
    expect(formatBytes("18446744073709551615")).toBe("15.9 EB");
  });

  it("reports percent only when the total is known", () => {
    expect(percentOf("512", "1024")).toBe("50.0%");
    expect(percentOf("512", null)).toBeNull();
    expect(percentOf("0", "0")).toBeNull();
  });

  it("distinguishes user pause from policy waits", () => {
    expect(statusText(snapshot({ desired_state: "paused", blockers: ["offline"] }), t)).toBe(
      "Paused",
    );
    expect(statusText(snapshot({ blockers: ["metered_network"] }), t)).toBe(
      "Waiting: metered network",
    );
    expect(statusText(snapshot({ downloaded_bytes: "1", total_bytes: "4" }), t)).toBe(
      "Downloading 25.0%",
    );
    expect(statusText(snapshot({ phase: "verifying" }), t)).toBe("Verifying");
  });
});

describe("categories", () => {
  it("files downloads by extension and phase", () => {
    const iso = snapshot({ display_name: "ubuntu.ISO", phase: "completed" });
    expect(matchesCategory(iso, "compressed")).toBe(true);
    expect(matchesCategory(iso, "finished")).toBe(true);
    expect(matchesCategory(iso, "unfinished")).toBe(false);
    expect(matchesCategory(snapshot({ display_name: "setup.exe" }), "programs")).toBe(true);
    expect(matchesCategory(snapshot({ display_name: "noextension" }), "documents")).toBe(false);
  });
});

describe("DownloadTable", () => {
  it("renders service snapshots without inventing rate or time left", () => {
    render(
      <PreferencesProvider>
        <DownloadTable
          categoryLabel="All downloads"
          connected
          downloads={[
            snapshot({
              display_name: "বাংলা-বই-সংগ্রহ-২০২৬.pdf",
              downloaded_bytes: "1048576",
              total_bytes: "2097152",
            }),
          ]}
        />
      </PreferencesProvider>,
    );
    const row = screen.getByRole("row", { name: /বাংলা-বই-সংগ্রহ-২০২৬\.pdf/ });
    expect(within(row).getByText("2.0 MB")).toBeVisible();
    expect(within(row).getByText("Downloading 50.0%")).toBeVisible();
    expect(within(row).getAllByText("—")).toHaveLength(3);
  });
});

describe("DownloadTable sorting", () => {
  const rows = [
    snapshot({ job_id: "job_b", display_name: "b-file10.zip", total_bytes: "300" }),
    snapshot({ job_id: "job_a", display_name: "a-file2.mp4", total_bytes: null }),
    snapshot({ job_id: "job_c", display_name: "c-file1.pdf", total_bytes: "100" }),
  ];
  const names = () =>
    screen.getAllByRole("rowheader").map((cell) => cell.textContent?.trim() ?? "");

  it("sorts by name and size from the column headers, unknown sizes last", async () => {
    const user = userEvent.setup();
    render(
      <PreferencesProvider>
        <DownloadTable categoryLabel="All downloads" connected downloads={rows} />
      </PreferencesProvider>,
    );
    expect(names()).toEqual(["b-file10.zip", "a-file2.mp4", "c-file1.pdf"]);

    await user.click(screen.getByRole("button", { name: "File name" }));
    expect(screen.getByRole("columnheader", { name: "File name" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
    expect(names()).toEqual(["a-file2.mp4", "b-file10.zip", "c-file1.pdf"]);

    await user.click(screen.getByRole("button", { name: "Size" }));
    expect(names()).toEqual(["c-file1.pdf", "b-file10.zip", "a-file2.mp4"]);
    await user.click(screen.getByRole("button", { name: "Size" }));
    expect(screen.getByRole("columnheader", { name: "Size" })).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    expect(names()).toEqual(["b-file10.zip", "c-file1.pdf", "a-file2.mp4"]);
    expect(screen.getByRole("columnheader", { name: "File name" })).not.toHaveAttribute(
      "aria-sort",
    );
  });

  it("does not offer sorting on columns without service data", () => {
    render(
      <PreferencesProvider>
        <DownloadTable categoryLabel="All downloads" connected downloads={rows} />
      </PreferencesProvider>,
    );
    expect(screen.queryByRole("button", { name: "Transfer rate" })).not.toBeInTheDocument();
  });

  it("marks rows with a file-type icon", () => {
    const { container } = render(
      <PreferencesProvider>
        <DownloadTable categoryLabel="All downloads" connected downloads={rows} />
      </PreferencesProvider>,
    );
    expect(container.querySelector(".download-table__type--compressed")).not.toBeNull();
    expect(container.querySelector(".download-table__type--video")).not.toBeNull();
    expect(container.querySelector(".download-table__type--documents")).not.toBeNull();
  });
});
