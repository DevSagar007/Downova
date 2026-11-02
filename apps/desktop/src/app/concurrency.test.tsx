import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, callError } from "../test/fake-service";

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
