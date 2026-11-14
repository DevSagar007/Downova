import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { HelloResult } from "@dm/contracts";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, HELLO, callError } from "../test/fake-service";
import { proxyProblem } from "../features/settings/ProxySettings";

const PASSWORD = "SYNTHETIC-proxy-pw-ui";

/** The default fake reports no capabilities; this one supports the HTTP proxy (F06). */
function supporting(fake = new FakeServiceClient(), proxy = true) {
  const hello = {
    ...HELLO,
    backend: {
      ...HELLO.backend,
      capabilities: proxy ? [{ capability: "http_proxy", support: "supported" }] : [],
    },
  } as unknown as HelloResult;
  fake.connection = { status: "connected", hello };
  return fake;
}

function setup(fake = supporting()) {
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

async function proxySection(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  const dialog = within(screen.getByRole("dialog", { name: "Options" }));
  return within(dialog.getByRole("region", { name: "Proxy" }));
}

describe("proxy draft validation (F06)", () => {
  it("follows the contract's rules", () => {
    const base = {
      mode: "http" as const,
      host: "p.test",
      port: "3128",
      username: "",
      password: "",
    };
    expect(proxyProblem(base)).toBeNull();
    expect(proxyProblem({ ...base, host: "http://p.test" })).toBe("proxy.invalid.host");
    expect(proxyProblem({ ...base, host: "p.test:3128" })).toBe("proxy.invalid.host");
    expect(proxyProblem({ ...base, host: "" })).toBe("proxy.invalid.host");
    expect(proxyProblem({ ...base, port: "0" })).toBe("proxy.invalid.port");
    expect(proxyProblem({ ...base, port: "70000" })).toBe("proxy.invalid.port");
    expect(proxyProblem({ ...base, username: "a:b" })).toBe("proxy.invalid.username");
    expect(proxyProblem({ ...base, password: "x" })).toBe("proxy.invalid.passwordNeedsUser");
    // Direct mode needs nothing, but keeps what it has valid.
    expect(proxyProblem({ ...base, mode: "direct", host: "", port: "" })).toBeNull();
  });
});

describe("Options → Proxy (F06)", () => {
  it("saves an HTTP proxy with a password, sends the password once and never shows it", async () => {
    const { fake, user } = setup();
    const section = await proxySection(user);
    const http = await section.findByRole("radio", { name: "Use an HTTP proxy" });
    await waitFor(() => expect(http).toBeEnabled());
    expect(section.getByRole("radio", { name: "No proxy: connect directly" })).toBeChecked();
    await user.click(http);
    await user.type(section.getByRole("textbox", { name: "Proxy host" }), "proxy.example.test");
    await user.type(section.getByRole("textbox", { name: "Port" }), "3128");
    await user.type(section.getByRole("textbox", { name: "User name (optional)" }), "alice");
    const password = section.getByLabelText("Password");
    expect(password).toHaveAttribute("type", "password");
    await user.type(password, PASSWORD);
    await user.click(section.getByRole("button", { name: "Save proxy settings" }));
    expect(await section.findByRole("status", { name: "Proxy" })).toHaveTextContent(
      "Saved: downloads use proxy.example.test:3128.",
    );
    expect(fake.proxyPasswords).toEqual([PASSWORD]);
    expect(JSON.stringify(fake.calls)).not.toContain(PASSWORD);
    expect(section.getByLabelText("Password")).toHaveValue("");
    expect(
      section.getByText(
        "A password is saved in the system credential store. Type a new one to replace it.",
      ),
    ).toBeVisible();
    expect(section.getByText("Carries: HTTP, HTTPS")).toBeVisible();
    // Saving again without typing keeps the saved password (the field is not sent).
    await user.click(section.getByRole("button", { name: "Save proxy settings" }));
    await waitFor(() =>
      expect(fake.calls.filter((c) => c.method === "proxySet").map((c) => c.args[4])).toEqual([
        "replace",
        "keep",
      ]),
    );
    // Removing it, then switching the proxy off: the fields stay saved.
    await user.click(section.getByRole("checkbox", { name: "Remove the saved password" }));
    await user.click(section.getByRole("radio", { name: "No proxy: connect directly" }));
    await user.click(section.getByRole("button", { name: "Save proxy settings" }));
    expect(await section.findByRole("status", { name: "Proxy" })).toHaveTextContent(
      "Saved: no proxy.",
    );
    expect(fake.proxy).toMatchObject({
      mode: "direct",
      host: "proxy.example.test",
      password_state: "none",
    });
  });

  it("refuses an invalid host without sending anything", async () => {
    const { fake, user } = setup();
    const section = await proxySection(user);
    const http = await section.findByRole("radio", { name: "Use an HTTP proxy" });
    await waitFor(() => expect(http).toBeEnabled());
    await user.click(http);
    await user.type(section.getByRole("textbox", { name: "Proxy host" }), "http://p.test");
    await user.type(section.getByRole("textbox", { name: "Port" }), "3128");
    expect(section.getByRole("alert")).toHaveTextContent(
      "Enter a host name or IP address, without http:// or a port.",
    );
    await user.click(section.getByRole("button", { name: "Save proxy settings" }));
    expect(fake.calls.some((c) => c.method === "proxySet")).toBe(false);
  });

  it("explains a store that refused the password and an engine without proxy support", async () => {
    const { fake, user } = setup();
    fake.next("proxySet", () =>
      Promise.reject(callError("rejected", "CREDENTIAL_STORE_UNAVAILABLE", "retry")),
    );
    const section = await proxySection(user);
    const http = await section.findByRole("radio", { name: "Use an HTTP proxy" });
    await waitFor(() => expect(http).toBeEnabled());
    await user.click(http);
    await user.type(section.getByRole("textbox", { name: "Proxy host" }), "10.0.0.8");
    await user.type(section.getByRole("textbox", { name: "Port" }), "8080");
    await user.type(section.getByRole("textbox", { name: "User name (optional)" }), "alice");
    await user.type(section.getByLabelText("Password"), PASSWORD);
    await user.click(section.getByRole("button", { name: "Save proxy settings" }));
    expect(
      await section.findByText(
        "The password could not be saved to the system credential store, so nothing was changed. Unlock the store and try again.",
      ),
    ).toBeVisible();
    expect(section.getByLabelText("Password")).toHaveValue("");
  });

  it("offers only direct when the engine cannot use a proxy", async () => {
    const { user } = setup(supporting(new FakeServiceClient(), false));
    const section = await proxySection(user);
    await waitFor(() =>
      expect(section.getByRole("radio", { name: "No proxy: connect directly" })).toBeEnabled(),
    );
    expect(section.getByRole("radio", { name: "Use an HTTP proxy" })).toBeDisabled();
    expect(section.getByText("This download engine cannot use a proxy.")).toBeVisible();
  });
});
