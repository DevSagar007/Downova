import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { AuthInfo, DownloadSnapshot } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, callError, snapshot } from "../test/fake-service";
import { bn, en } from "../i18n/messages";

const ID = "cred_0123456789abcdef";

function auth(over: Partial<AuthInfo> = {}): AuthInfo {
  return {
    credential_id: ID,
    origin: "https://files.example.org",
    browser: "chrome",
    mode: "temporary",
    state: "available",
    ...over,
  };
}

function job(a: AuthInfo, over: Partial<DownloadSnapshot> = {}): DownloadSnapshot {
  return snapshot({
    job_id: "job_signed",
    display_name: "report.pdf",
    source_display: "https://files.example.org/…/report.pdf",
    phase: "transferring",
    auth: a,
    ...over,
  });
}

async function details(j: DownloadSnapshot, fake = new FakeServiceClient()) {
  fake.streaming = true;
  fake.items = [j];
  fake.credentials = j.auth ? [{ ...j.auth, created_at_ms: "1", active_jobs: 1 }] : [];
  render(
    <PreferencesProvider>
      <ServiceProvider client={fake} shell={new FakeShell()}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "report.pdf" }));
  return { fake, user };
}

describe("signed-in downloads (BRW-05, SEC-02)", () => {
  it("names the site, browser and mode, never a secret, and the link only in display form", async () => {
    await details(job(auth({ mode: "remembered", browser: "edge" })));
    expect(
      await screen.findByText("Uses your sign-in for https://files.example.org"),
    ).toBeVisible();
    expect(screen.getByText(/From Edge\. Remembered in the system credential store/)).toBeVisible();
    expect(screen.getByText("Available.")).toBeVisible();
    expect(document.body.textContent).not.toMatch(/sid=|cookie|token=/i);
  });

  it("forgets once however fast the clicks come, and the download shows it as forgotten", async () => {
    const { fake } = await details(job(auth()));
    const forget = await screen.findByRole("button", { name: "Forget sign-in" });
    act(() => {
      forget.click();
      forget.click();
    });
    expect(
      await screen.findByText("The sign-in was forgotten. Downloads that used it are paused."),
    ).toBeVisible();
    expect(fake.count("credentialsForget")).toBe(1);
    expect(fake.calls.find((c) => c.method === "credentialsForget")?.args).toEqual([ID]);
    expect(await screen.findByText(/^Forgotten\./)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Forget sign-in" })).toBeNull();
  });

  it("explains a failed forget and keeps the action", async () => {
    const fake = new FakeServiceClient();
    fake.next("credentialsForget", async () => {
      throw callError("unavailable");
    });
    const { user } = await details(job(auth()), fake);
    await user.click(await screen.findByRole("button", { name: "Forget sign-in" }));
    expect(await screen.findByText("The sign-in could not be forgotten.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Forget sign-in" })).toBeEnabled();
  });

  it("an expired sign-in says how to reconnect from the browser", async () => {
    await details(
      job(auth({ state: "expired" }), {
        phase: "failed",
        error: { kind: "AUTH_REQUIRED", retryable: false, next_action: "sign_in" },
      }),
    );
    expect(await screen.findByText(/The website no longer accepts this sign-in/)).toBeVisible();
    const banner = screen.getByText(/Download with Downova using my sign-in”, or download it/);
    expect(banner).toBeVisible();
  });

  it("a locked store asks for an unlock", async () => {
    await details(
      job(auth({ mode: "remembered", state: "locked" }), {
        phase: "failed",
        error: { kind: "CREDENTIAL_STORE_UNAVAILABLE", retryable: true, next_action: "retry" },
      }),
    );
    const note = await screen.findByText(/The system credential store is locked\. Unlock it/);
    expect(within(note.closest("div") ?? document.body).getByText(/Unlock it/)).toBeVisible();
  });

  it("has every sign-in string in Bangla", () => {
    const keys = Object.keys(en).filter(
      (k) =>
        k.startsWith("auth.") ||
        k === "error.kind.AUTH_SCOPE_UNSUPPORTED" ||
        k === "error.kind.CREDENTIAL_STORE_UNAVAILABLE" ||
        k === "error.action.sign_in",
    );
    expect(keys.length).toBeGreaterThanOrEqual(18);
    for (const k of keys) {
      expect(bn[k as keyof typeof bn], k).toBeTruthy();
      expect(bn[k as keyof typeof bn], k).not.toBe(en[k as keyof typeof en]);
    }
  });
});
