// B2.16–B2.19: Retry, Refresh link, checksum input and presentation, Browse… and Open containing
// folder, against the fake service. Behaviour only: keyboard, eligibility, duplicate submissions,
// uncertain answers, cancellation and language.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { callError, deferred, FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";

function setup(items = [] as ReturnType<typeof snapshot>[], shell = new FakeShell()) {
  const fake = new FakeServiceClient();
  fake.streaming = true;
  fake.items = items;
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={shell} reconnectDelaysMs={[20]}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  return { fake, shell, user: userEvent.setup() };
}

const failed = (over: Partial<ReturnType<typeof snapshot>> = {}) =>
  snapshot({
    job_id: "job_fail",
    attempt_id: "att_old",
    display_name: "broken.iso",
    phase: "failed",
    downloaded_bytes: "1048576",
    error: {
      kind: "REMOTE_CHANGED",
      retryable: false,
      next_action: "choose_restart_or_keep",
      detail: null,
    },
    ...over,
  });

async function select(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(await screen.findByRole("button", { name }));
}

async function fileMenu(user: ReturnType<typeof userEvent.setup>, item: string) {
  await user.click(screen.getByRole("menuitem", { name: "File" }));
  return screen.getByRole("menuitem", { name: item });
}

describe("Retry (CORE-03)", () => {
  it("says the partial data is discarded and sends one operation however often it is pressed", async () => {
    const { fake, user } = setup([failed()]);
    await select(user, "broken.iso");
    await user.click(screen.getByRole("button", { name: "Retry…" }));
    const dialog = screen.getByRole("dialog", { name: "Retry this download?" });
    expect(dialog).toHaveTextContent(/1.0 MB already downloaded is discarded/);
    const gate = deferred<ReturnType<typeof snapshot>>();
    fake.next("retry", () => gate.promise);
    const confirm = within(dialog).getByRole("button", { name: "Retry from the beginning" });
    await user.click(confirm);
    await user.click(within(dialog).getByRole("button", { name: "Retrying…" }));
    expect(fake.count("retry")).toBe(1);
    gate.resolve(failed({ phase: "queued", error: null, downloaded_bytes: "0" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(fake.lastRetry).toEqual({ expectedAttemptId: "att_old", discardPartial: true });
  });

  it("repeats an unconfirmed retry with the same operation ID", async () => {
    const { fake, user } = setup([failed()]);
    await select(user, "broken.iso");
    await user.click(screen.getByRole("button", { name: "Retry…" }));
    const dialog = screen.getByRole("dialog");
    fake.next("retry", () => Promise.reject(callError("uncertain")));
    await user.click(within(dialog).getByRole("button", { name: "Retry from the beginning" }));
    expect(await within(dialog).findByText(/never starts twice/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Retry from the beginning" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const [first, second] = fake.calls.filter((c) => c.method === "retry");
    expect(first?.args[1]).toBe(second?.args[1]);
    expect(fake.attempts).toBe(1);
  });

  it("Keep as it is sends nothing; Retry is unavailable with reasons elsewhere", async () => {
    const expired = failed({
      job_id: "job_exp",
      display_name: "expired.zip",
      error: {
        kind: "LINK_REFRESH_REQUIRED",
        retryable: false,
        next_action: "refresh_link",
        detail: null,
      },
    });
    const { fake, user } = setup([
      failed(),
      expired,
      snapshot({ job_id: "job_ok", display_name: "done.pdf", phase: "completed" }),
    ]);
    await select(user, "broken.iso");
    await user.click(screen.getByRole("button", { name: "Retry…" }));
    await user.click(screen.getByRole("button", { name: "Keep as it is" }));
    expect(fake.count("retry")).toBe(0);
    await select(user, "done.pdf");
    expect(await fileMenu(user, "Retry…")).toHaveAccessibleDescription(
      /only a download that stopped with an error/,
    );
    await user.keyboard("{Escape}");
    await select(user, "expired.zip");
    expect(await fileMenu(user, "Retry…")).toHaveAccessibleDescription(/Refresh link/);
    expect(screen.queryByRole("button", { name: "Retry…" })).toBeNull();
    expect(screen.getByRole("button", { name: "Refresh link…" })).toBeVisible();
  });
});

describe("Refresh link (COR-06)", () => {
  const paused = snapshot({
    job_id: "job_pause",
    attempt_id: "att_p1",
    display_name: "big.iso",
    desired_state: "paused",
    phase: "transferring",
    downloaded_bytes: "2097152",
  });

  it("validates the link, keeps proven progress and never shows the link elsewhere", async () => {
    const { fake, user } = setup([paused]);
    await select(user, "big.iso");
    await user.click(await fileMenu(user, "Refresh link…"));
    const dialog = screen.getByRole("dialog", { name: "Refresh link" });
    const field = within(dialog).getByLabelText("New link");
    expect(field).toHaveFocus();
    await user.type(field, "ftp://x/big.iso{Enter}");
    expect(within(dialog).getByRole("alert")).toHaveTextContent(/http:\/\/ or https:\/\//);
    expect(fake.count("replaceSource")).toBe(0);
    await user.clear(field);
    await user.type(field, "https://example.org/big.iso?sig=SECRET{Enter}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(fake.calls.find((c) => c.method === "replaceSource")?.args[3]).toBe("keep_if_verified");
    expect(document.body.textContent).not.toContain("SECRET");
  });

  it("offers Restart or Keep current when progress cannot be kept; Keep sends nothing more", async () => {
    const { fake, user } = setup([paused]);
    fake.replaceAnswer = { outcome: "choice_required", reason: "content_differs" };
    await select(user, "big.iso");
    await user.click(screen.getByRole("button", { name: "Refresh link…" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByLabelText("New link"), "https://mirror.example/b.iso");
    await user.click(within(dialog).getByRole("button", { name: "Use new link" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent(/data already downloaded differs/);
    expect(alert).toHaveTextContent(/2.0 MB already downloaded for this download only/);
    await user.click(within(dialog).getByRole("button", { name: "Keep current download" }));
    expect(fake.count("replaceSource")).toBe(1);
    expect(fake.items[0]?.downloaded_bytes).toBe("2097152");
  });

  it("restarts only on the explicit choice, with its own operation", async () => {
    const { fake, user } = setup([paused]);
    fake.replaceAnswer = { outcome: "choice_required", reason: "different_resource" };
    await select(user, "big.iso");
    await user.click(screen.getByRole("button", { name: "Refresh link…" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByLabelText("New link"), "https://mirror.example/b.iso");
    await user.click(within(dialog).getByRole("button", { name: "Use new link" }));
    await user.click(
      await within(dialog).findByRole("button", { name: "Restart from zero with the new link" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const calls = fake.calls.filter((c) => c.method === "replaceSource");
    expect(calls.map((c) => c.args[3])).toEqual(["keep_if_verified", "restart"]);
    expect(calls[0]?.args[1]).not.toBe(calls[1]?.args[1]);
    expect(fake.items[0]?.downloaded_bytes).toBe("0");
  });

  it("is unavailable for a running download until it is paused", async () => {
    const { user } = setup([snapshot({ job_id: "job_run", display_name: "run.bin" })]);
    await select(user, "run.bin");
    expect(await fileMenu(user, "Refresh link…")).toHaveAccessibleDescription(/pause the download/);
  });
});

describe("checksum input and presentation (COR-05)", () => {
  it("accepts exactly 64 hex digits and sends the checksum only when given", async () => {
    const { fake, user } = setup();
    await user.click(await screen.findByRole("button", { name: "Add URL" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByLabelText("Address (URL)"), "https://h/a.iso");
    const sha = within(dialog).getByLabelText("Publisher's SHA-256 (optional)");
    await user.type(sha, "abc");
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(/exactly 64 characters/);
    expect(sha).toHaveAttribute("aria-invalid", "true");
    await user.clear(sha);
    await user.type(sha, `  ${"AB".repeat(32)} `);
    await user.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fake.count("add")).toBe(1));
    expect(fake.lastAddOptions?.expectedSha256).toBe("ab".repeat(32));
  });

  it("separates a publisher match from a locally computed hash", async () => {
    const hash = "c".repeat(64);
    const { user } = setup([
      snapshot({
        job_id: "job_pub",
        display_name: "pub.iso",
        phase: "completed",
        verification: { sha256: hash, source: "publisher_checksum" },
      }),
      snapshot({
        job_id: "job_loc",
        display_name: "loc.iso",
        phase: "completed",
        verification: { sha256: hash, source: "local_hash" },
      }),
    ]);
    await select(user, "pub.iso");
    expect(
      await screen.findByRole("heading", { name: "Publisher checksum matched" }),
    ).toBeVisible();
    await select(user, "loc.iso");
    const heading = await screen.findByRole("heading", {
      name: "SHA-256 computed locally (no publisher checksum)",
    });
    const details = heading.closest("section");
    expect(details).toHaveTextContent(/does not prove it is the publisher/);
    expect(details).not.toHaveTextContent("Publisher checksum matched");
  });

  it("explains a mismatch with the recovery actions, in Bangla too", async () => {
    const { user } = setup([
      failed({
        error: {
          kind: "CHECKSUM_MISMATCH",
          retryable: false,
          next_action: "none",
          detail: null,
        },
        checksum_expected: true,
      }),
    ]);
    await select(user, "broken.iso");
    expect(await screen.findByText(/received file is not the publisher's/)).toBeVisible();
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    await user.click(await screen.findByRole("menuitemradio", { name: /বাংলা/ }));
    expect(await screen.findByText(/প্রকাশকের পাতায় SHA-256 মিলিয়ে দেখুন/)).toBeVisible();
    expect(screen.getByRole("button", { name: "আবার চেষ্টা…" })).toBeVisible();
  });
});

describe("folders (CORE-07, ADV12)", () => {
  async function openOptions(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole("button", { name: "Options" }));
    return screen.getByRole("dialog", { name: "Options" });
  }

  it("Browse… fills the field; cancelling the dialog changes nothing and saves nothing", async () => {
    const { fake, shell, user } = setup();
    fake.settings = {
      destination_rules: [{ category: "video", directory: "D:\\Videos" }],
      on_collision: "rename",
    };
    const dialog = await openOptions(user);
    const browse = await within(dialog).findByRole("button", {
      name: "Choose the folder for Video",
    });
    shell.folderAnswer = null;
    await user.click(browse);
    const field = within(dialog).getByLabelText("Video");
    expect(field).toHaveValue("D:\\Videos");
    expect(shell.pickedFrom).toEqual(["D:\\Videos"]);
    expect(fake.count("settingsSet")).toBe(0);
    shell.folderAnswer = "E:\\Films";
    await user.click(browse);
    expect(field).toHaveValue("E:\\Films");
    expect(fake.count("settingsSet")).toBe(0);
  });

  it("Open containing folder is a keyboard-reachable user action with honest outcomes", async () => {
    const { fake, user } = setup([
      snapshot({ job_id: "job_done", display_name: "done.pdf", phase: "completed" }),
    ]);
    await select(user, "done.pdf");
    expect(fake.count("reveal")).toBe(0);
    const open = await fileMenu(user, "Open containing folder");
    await user.click(open);
    expect(fake.calls.find((c) => c.method === "reveal")?.args).toEqual(["job_done"]);
    fake.revealAnswer = "folder_file_missing";
    screen.getByRole("button", { name: "Open containing folder" }).focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByText(/no longer in it/)).toBeVisible();
    fake.next("reveal", () =>
      Promise.reject(callError("rejected", "VOLUME_MISSING", "reconnect_volume")),
    );
    await user.click(await fileMenu(user, "Open containing folder"));
    expect(await screen.findByText("The folder could not be opened")).toBeVisible();
    expect(screen.getByText(/Reconnect the drive/)).toBeVisible();
  });
});
