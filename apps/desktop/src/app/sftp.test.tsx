import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";
import { presentedKey, sftpServer } from "../features/downloads/HostKeyNote";

const FP = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";

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

function stopped(kind: "HOST_KEY_UNKNOWN" | "HOST_KEY_CHANGED") {
  return snapshot({
    job_id: "job_sftp",
    display_name: "image.iso",
    source_display: "sftp://Files.Example.org/srv/image.iso",
    phase: "failed",
    error: {
      kind,
      retryable: false,
      next_action: kind === "HOST_KEY_UNKNOWN" ? "verify_server" : "none",
      detail: `the key is not trusted yet (server key ssh-ed25519 ${FP})`,
    },
  });
}

describe("SFTP host keys (F05, ADR 0037)", () => {
  it("reads the key and the server from the download", () => {
    expect(presentedKey(`x (server key ssh-ed25519 ${FP})`)).toEqual({
      algorithm: "ssh-ed25519",
      fingerprint: FP,
    });
    expect(presentedKey("no key")).toBeNull();
    expect(sftpServer("sftp://Files.Example.org:2222/a")).toEqual({
      host: "files.example.org",
      port: 2222,
    });
    expect(sftpServer("sftp://h/a")).toEqual({ host: "h", port: 22 });
    expect(sftpServer("https://h/a")).toBeNull();
  });

  it("shows an unknown key and trusts exactly that key on request", async () => {
    const fake = new FakeServiceClient();
    fake.items = [stopped("HOST_KEY_UNKNOWN")];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "image.iso" }));
    expect(await screen.findByText("Check the key of files.example.org:22")).toBeVisible();
    expect(screen.getByText(`ssh-ed25519 ${FP}`)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Trust this key and retry" }));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "knownHostTrust").map((c) => c.args)).toEqual([
        ["files.example.org", "22", FP],
      ]),
    );
    expect(await screen.findByText("The key is trusted for this server.")).toBeVisible();
  });

  it("never offers to trust a changed key", async () => {
    const fake = new FakeServiceClient();
    fake.items = [stopped("HOST_KEY_CHANGED")];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "image.iso" }));
    expect(await screen.findByText("The key of files.example.org:22 changed")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Trust this key/ })).toBeNull();
    expect(screen.getByText(/remove the server in Options → Known servers/)).toBeVisible();
  });

  it("saves an SFTP site login on port 22 and lists and forgets known servers", async () => {
    const fake = new FakeServiceClient();
    fake.knownHosts = [
      {
        host: "files.example.org",
        port: 22,
        algorithm: "ssh-ed25519",
        fingerprint: FP,
        state: "trusted",
      },
    ];
    const { user } = setup(fake);
    await user.click(await screen.findByRole("button", { name: "Options" }));
    const dialog = within(screen.getByRole("dialog", { name: "Options" }));
    const logins = within(dialog.getByRole("region", { name: "Site logins" }));
    await logins.findByText(/No site logins yet/);
    await user.selectOptions(logins.getByRole("combobox", { name: "Protocol" }), "sftp");
    expect(logins.getByRole("textbox", { name: "Port" })).toHaveValue("22");
    await user.type(logins.getByRole("textbox", { name: "Server" }), "files.example.org");
    await user.type(logins.getByRole("textbox", { name: "User name" }), "alice");
    await user.type(logins.getByLabelText("Password"), "SYNTHETIC-pw");
    await user.click(logins.getByRole("button", { name: "Save site login" }));
    expect(await logins.findByText(/sftp:\/\/files\.example\.org:22/)).toBeVisible();

    const known = within(dialog.getByRole("region", { name: "Known servers" }));
    expect(await known.findByText(`ssh-ed25519 ${FP}`)).toBeVisible();
    await user.click(known.getByRole("button", { name: "Forget the key of files.example.org:22" }));
    expect(await known.findByText("No server keys are trusted yet.")).toBeVisible();
  });
});
