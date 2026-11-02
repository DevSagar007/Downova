import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, HELLO, callError } from "../test/fake-service";

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

async function open(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Options" }));
  return within(screen.getByRole("dialog", { name: "Options" }));
}

describe("global download concurrency", () => {
  it("loads service truth and saves six once without silently changing a queue", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    expect(field).toHaveAccessibleDescription(/queue.*Manage queues/i);
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    expect(fake.count("concurrencySet")).toBe(1);
    expect(fake.count("queueLimit")).toBe(0);
  });

  it("rejects empty, fractional and out-of-range input without submitting", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    for (const value of ["", "0", "17", "2.5"]) {
      await user.clear(field);
      if (value) await user.type(field, value);
      await user.click(dialog.getByRole("button", { name: "Save download limit" }));
      expect(field).toHaveAttribute("aria-invalid", "true");
    }
    expect(fake.count("concurrencySet")).toBe(0);
  });

  it("holds one pending submission and keeps an uncertain outcome explicit", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let reject!: (e: unknown) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((_, no) => {
          reject = no;
        }),
    );
    await user.clear(field);
    await user.type(field, "10");
    await user.dblClick(dialog.getByRole("button", { name: "Save download limit" }));
    expect(fake.count("concurrencySet")).toBe(1);
    expect(field).toBeDisabled();
    await act(async () => reject(callError("uncertain")));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not confirm/i);
    expect(field).toHaveValue(10);
    fake.concurrency = { max_active_downloads: 10 };
    let resolveRead!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencyGet",
      () =>
        new Promise((resolve) => {
          resolveRead = resolve;
        }),
    );
    await user.click(dialog.getByRole("button", { name: "Reload download limit" }));
    expect(field).toBeDisabled();
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(fake.count("concurrencySet")).toBe(1);
    await act(async () => resolveRead({ max_active_downloads: 10 }));
    await waitFor(() => expect(dialog.queryByRole("alert")).not.toBeInTheDocument());
    expect(field).toHaveValue(10);
    expect(fake.count("concurrencySet"), "reading never repeats a save").toBe(1);
  });

  it("a rejected save never reports the rejected value as saved", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    fake.next("concurrencySet", () => Promise.reject(callError("rejected", "INTERNAL", "retry")));
    await user.clear(field);
    await user.type(field, "9");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not be read or saved/i);
    // The earlier "saved" status is gone and nothing claims 9 was saved.
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
    expect(fake.concurrency.max_active_downloads).toBe(6);
  });

  it("a rejected re-save of an unchanged value drops the earlier saved status", async () => {
    // Typing clears the status on its own; saving the same value again must clear it too.
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await user.clear(field);
    await user.type(field, "6");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("status", { name: "Download limit" })).toHaveTextContent("6");
    fake.next("concurrencySet", () => Promise.reject(callError("rejected", "INTERNAL", "retry")));
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent(/could not be read or saved/i);
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("ignores a save reply from a service instance the window has since reconnected away from", async () => {
    const { fake, user } = setup();
    const dialog = await open(user);
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let resolveSave!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    await user.clear(field);
    await user.type(field, "12");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    // The service goes away and a new instance answers; the open dialog reads it afresh.
    fake.connection = { status: "unavailable", reason: "not_running", error: null };
    act(() => fake.endStream("lost"));
    await waitFor(() => expect(field).toBeDisabled());
    fake.connection = { status: "connected", hello: { ...HELLO, service_instance_id: "svc_new" } };
    fake.probeAnswer = fake.connection;
    await waitFor(() => expect(field).toHaveValue(3), { timeout: 5000 });
    // The old instance's reply arrives late: it must not replace what the new one reported.
    await act(async () => resolveSave({ max_active_downloads: 12 }));
    expect(field).toHaveValue(3);
    expect(field).toBeEnabled();
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("ignores a save reply that arrives after the dialog was closed and opened again", async () => {
    const { fake, user } = setup();
    let dialog = await open(user);
    let field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    let resolveSave!: (value: { max_active_downloads: number }) => void;
    fake.next(
      "concurrencySet",
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    await user.clear(field);
    await user.type(field, "12");
    await user.click(dialog.getByRole("button", { name: "Save download limit" }));
    await user.click(dialog.getByRole("button", { name: "Close" }));
    // Reopened: the dialog reads the service again (still 3), then the old reply lands.
    dialog = await open(user);
    field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    await act(async () => resolveSave({ max_active_downloads: 12 }));
    expect(field).toHaveValue(3);
    expect(dialog.queryByRole("status", { name: "Download limit" })).not.toBeInTheDocument();
  });

  it("offers a read retry when the service setting cannot be loaded", async () => {
    const fake = new FakeServiceClient();
    fake.next("concurrencyGet", () => Promise.reject(callError("unavailable")));
    const { user } = setup(fake);
    const dialog = await open(user);
    await user.click(await dialog.findByRole("button", { name: "Reload download limit" }));
    const field = await dialog.findByRole("spinbutton", { name: "Maximum active downloads" });
    await waitFor(() => expect(field).toHaveValue(3));
    expect(fake.count("concurrencySet")).toBe(0);
  });

  it("localizes the label and validation in Bangla", async () => {
    const { user } = setup();
    const dialog = await open(user);
    await user.selectOptions(dialog.getByRole("combobox", { name: "Language" }), "bn");
    expect(
      await screen.findByRole("spinbutton", { name: "সর্বোচ্চ সক্রিয় ডাউনলোড" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "ডাউনলোডের সীমা সংরক্ষণ" })).toBeVisible();
  });
});
