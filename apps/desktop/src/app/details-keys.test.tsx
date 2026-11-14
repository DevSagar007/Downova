import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell, snapshot } from "../test/fake-service";

afterEach(() => {
  vi.restoreAllMocks();
});

const duplicateKeyWarnings = (spy: { mock: { calls: unknown[][] } }) =>
  spy.mock.calls.filter((args) => String(args[0]).includes("two children with the same key"));

describe("download details", () => {
  // The P2 "two children with the same key" warning: the speed limit control and the sign-in
  // note were siblings keyed by the same job ID, so every unfinished selected download (one just
  // added included) logged it and React could drop one of them.
  it.each(["transferring", "queued"] as const)(
    "a selected %s download shows its controls without duplicate React keys",
    async (phase) => {
      const errors = vi.spyOn(console, "error");
      const user = userEvent.setup();
      const fake = new FakeServiceClient();
      fake.items = [snapshot({ job_id: "job_a", display_name: "a.bin", phase })];
      render(
        <PreferencesProvider>
          <ServiceProvider client={fake} shell={new FakeShell()}>
            <App />
          </ServiceProvider>
        </PreferencesProvider>,
      );
      await user.click(await screen.findByRole("button", { name: "a.bin" }));
      expect(await screen.findByRole("group", { name: /No limit of its own/ })).toBeVisible();
      expect(duplicateKeyWarnings(errors)).toEqual([]);
    },
  );
});
