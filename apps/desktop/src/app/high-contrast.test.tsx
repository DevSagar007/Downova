import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell } from "../test/fake-service";

function renderApp() {
  return render(
    <PreferencesProvider>
      <ServiceProvider client={new FakeServiceClient()} shell={new FakeShell()}>
        <App />
      </ServiceProvider>
    </PreferencesProvider>,
  );
}

afterEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("high-contrast theme (F16)", () => {
  it("is chosen from Options, applied as a token set and remembered after a restart", async () => {
    const user = userEvent.setup();
    const view = renderApp();
    await user.click(await screen.findByRole("button", { name: "Options" }));
    const dialog = within(screen.getByRole("dialog", { name: "Options" }));
    await user.selectOptions(dialog.getByRole("combobox", { name: "Theme" }), "High contrast");
    expect(document.documentElement.dataset.theme).toBe("high-contrast");
    view.unmount();
    delete document.documentElement.dataset.theme;
    renderApp();
    expect(document.documentElement.dataset.theme).toBe("high-contrast");
  });

  it("is offered in the View menu as a radio item, in English and Bangla", async () => {
    const user = userEvent.setup();
    renderApp();
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    const menu = screen.getByRole("menu", { name: "View" });
    const item = within(menu).getByRole("menuitemradio", { name: "Theme: high contrast" });
    await user.click(item);
    expect(document.documentElement.dataset.theme).toBe("high-contrast");
    await user.click(screen.getByRole("menuitem", { name: "View" }));
    expect(
      within(screen.getByRole("menu", { name: "View" })).getByRole("menuitemradio", {
        name: "Theme: high contrast",
      }),
    ).toHaveAttribute("aria-checked", "true");
    await user.click(
      within(screen.getByRole("menu", { name: "View" })).getByRole("menuitemradio", {
        name: "বাংলা (Bangla)",
      }),
    );
    await user.click(screen.getByRole("menuitem", { name: "দেখুন" }));
    expect(screen.getByRole("menuitemradio", { name: "থিম: উচ্চ কনট্রাস্ট" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("an unknown stored theme falls back to the system theme", () => {
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ theme: "neon-skin" }));
    renderApp();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
