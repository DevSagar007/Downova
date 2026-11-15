import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, callError } from "../test/fake-service";

const PASSWORD = "SYNTHETIC-site-login-ui";

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

async function section(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  const dialog = within(screen.getByRole("dialog", { name: "Options" }));
  return within(dialog.getByRole("region", { name: "Site logins" }));
}

describe("Options → Site logins (F05, ADR 0036)", () => {
  it("saves a login, sends the password once, keeps it on an update and removes it", async () => {
    const { fake, user } = setup();
    const s = await section(user);
    expect(await s.findByText(/No site logins yet/)).toBeVisible();
    await user.type(s.getByRole("textbox", { name: "Server" }), "Files.Example.org");
    await user.type(s.getByRole("textbox", { name: "User name" }), "alice");
    await user.type(s.getByLabelText("Password"), PASSWORD);
    await user.click(s.getByRole("button", { name: "Save site login" }));
    expect(await s.findByRole("status", { name: "Site logins" })).toHaveTextContent(
      "Saved the site login for files.example.org:21.",
    );
    expect(fake.siteLoginPasswords).toEqual([PASSWORD]);
    expect(JSON.stringify(fake.calls)).not.toContain(PASSWORD);
    const list = s.getByRole("list", { name: "Saved site logins" });
    expect(within(list).getByText(/ftp:\/\/files\.example\.org:21/)).toBeVisible();
    expect(within(list).getByText(/password saved/)).toBeVisible();
    expect(s.getByLabelText("Password")).toHaveValue("");

    // An update without a password keeps the saved one.
    await user.type(s.getByRole("textbox", { name: "Server" }), "files.example.org");
    await user.type(s.getByRole("textbox", { name: "User name" }), "bob");
    expect(s.getByText(/leave the field empty to keep it/)).toBeVisible();
    await user.click(s.getByRole("button", { name: "Save site login" }));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "siteLoginSet").map((c) => c.args[3])).toEqual([
        "replace",
        "keep",
      ]),
    );
    expect(await within(list).findByText(/user bob/)).toBeVisible();

    await user.click(
      s.getByRole("button", { name: "Remove the site login for files.example.org:21" }),
    );
    expect(await s.findByText(/No site logins yet/)).toBeVisible();
    expect(fake.siteLogins).toEqual([]);
  });

  it("asks for a password for a new server and explains a refused store", async () => {
    const { fake, user } = setup();
    const s = await section(user);
    await s.findByText(/No site logins yet/);
    await user.type(s.getByRole("textbox", { name: "Server" }), "h.example");
    await user.type(s.getByRole("textbox", { name: "User name" }), "alice");
    expect(s.getByRole("alert")).toHaveTextContent("Enter the password for a new site login.");
    await user.click(s.getByRole("button", { name: "Save site login" }));
    expect(fake.calls.some((c) => c.method === "siteLoginSet")).toBe(false);

    fake.next("siteLoginSet", () =>
      Promise.reject(callError("rejected", "CREDENTIAL_STORE_UNAVAILABLE", "retry")),
    );
    await user.type(s.getByLabelText("Password"), PASSWORD);
    await user.click(s.getByRole("button", { name: "Save site login" }));
    expect(await s.findByText(/could not be saved to the system credential store/)).toBeVisible();
    expect(s.getByLabelText("Password")).toHaveValue("");
  });
});
