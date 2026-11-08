// REL-03: Help → Third-party notices… is offline (bundled inventory), shows license texts
// verbatim in their original language, records incomplete metadata instead of hiding it, and
// speaks English and Bangla around the texts.
import { afterEach, describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { PreferencesProvider } from "./preferences";
import { ServiceProvider } from "../services/ServiceProvider";
import { FakeServiceClient, FakeShell } from "../test/fake-service";
import { loadNotices, NoticesDialog, type NoticeData } from "../features/support/NoticesDialog";

const SAMPLE: NoticeData = {
  summary: { distributed: 2, unresolved: 1 },
  components: [
    {
      name: "alpha",
      version: "1.2.3",
      ecosystem: "cargo",
      license: "MIT",
      source: "registry",
      repository: "https://example.invalid/alpha",
      targets: ["windows-x86_64", "linux-x86_64"],
      texts: ["t1"],
      unresolved: [],
    },
    {
      name: "beta",
      version: null,
      ecosystem: "installer",
      license: null,
      source: "installer runtime",
      repository: null,
      targets: ["windows-x86_64 (NSIS)"],
      texts: [],
      unresolved: ["version and license are not recorded in the binary"],
    },
  ],
  external: [{ name: "aria2", version: "1.37.0", reason: "not bundled" }],
  texts: {
    t1: {
      file: "alpha-1.2.3/LICENSE",
      text: "Copyright (c) Alpha\n\n    indented   spacing kept\n",
    },
  },
};

function renderDialog(load: () => Promise<NoticeData> = async () => SAMPLE) {
  return render(
    <PreferencesProvider>
      <NoticesDialog onClose={() => {}} load={load} />
    </PreferencesProvider>,
  );
}

afterEach(() => localStorage.removeItem("dm.ui.preferences.v1"));

describe("third-party notices", () => {
  it("opens from the Help menu and loads the bundled inventory", async () => {
    const user = userEvent.setup();
    render(
      <PreferencesProvider>
        <ServiceProvider
          client={new FakeServiceClient()}
          shell={new FakeShell()}
          pollIntervalMs={50}
        >
          <App />
        </ServiceProvider>
      </PreferencesProvider>,
    );
    await user.click(screen.getByRole("menuitem", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "Third-party notices…" }));
    const dialog = screen.getByRole("dialog", { name: "Third-party notices" });
    const list = await within(dialog).findByRole("list", { name: "Components" }, { timeout: 5000 });
    expect(
      within(list).getByRole("button", { name: /^react 19\.3\.1,\s*MIT$/ }),
    ).toBeInTheDocument();
  });

  it("the bundled inventory names every component with targets and existing texts", async () => {
    const data = await loadNotices();
    expect(data.components.length).toBe(data.summary.distributed);
    for (const c of data.components) {
      expect(c.targets.length).toBeGreaterThan(0);
      for (const id of c.texts) expect(data.texts[id]?.text.length).toBeGreaterThan(0);
      if (!c.license || c.texts.length === 0) expect(c.unresolved.length).toBeGreaterThan(0);
    }
    // The production web dependencies and the statically linked Rust runtime are listed.
    const names = data.components.map((c) => c.name);
    for (const n of [
      "react",
      "react-dom",
      "@tauri-apps/api",
      "lucide-react",
      "tauri",
      "Rust standard library",
    ])
      expect(names).toContain(n);
    // aria2 is not bundled (ADR 0005), so it is never a distributed component.
    expect(names).not.toContain("aria2");
    expect(data.external.map((e) => e.name)).toContain("aria2");
  });

  it("shows a license text verbatim, left to right, in its original language", async () => {
    const user = userEvent.setup();
    renderDialog();
    const toggle = await screen.findByRole("button", { name: /alpha 1\.2\.3/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const text = screen.getByRole("textbox", { name: "alpha-1.2.3/LICENSE" });
    expect(text).toHaveValue(SAMPLE.texts.t1?.text);
    expect(text).toHaveAttribute("readonly");
    expect(text).toHaveAttribute("lang", "en");
    expect(text).toHaveAttribute("dir", "ltr");
    expect(screen.getByText("windows-x86_64, linux-x86_64")).toBeInTheDocument();
  });

  it("states unknown versions, licenses and missing texts instead of filling them in", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(
      await screen.findByRole("button", {
        name: /beta \(version unknown\),\s*License not recorded/,
      }),
    );
    expect(screen.getByText("Incomplete license information:")).toBeInTheDocument();
    expect(
      screen.getByText("version and license are not recorded in the binary"),
    ).toBeInTheDocument();
    expect(screen.getByText("The published package contains no license text.")).toBeInTheDocument();
    expect(
      screen.getByText(/2 components; 1 of them have incomplete license information/),
    ).toBeInTheDocument();
  });

  it("filters by name or license and says when nothing matches", async () => {
    const user = userEvent.setup();
    renderDialog();
    const filter = await screen.findByRole("searchbox", { name: "Filter by name or license" });
    await user.type(filter, "mit");
    expect(screen.getByRole("button", { name: /alpha/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /beta/ })).toBeNull();
    await user.clear(filter);
    await user.type(filter, "zzz");
    expect(screen.getByText("No component matches this filter.")).toBeInTheDocument();
  });

  it("explains an unreadable inventory", async () => {
    renderDialog(() => Promise.reject(new Error("x")));
    expect(
      await screen.findByText("The notices bundled with this copy of Downova could not be read."),
    ).toBeInTheDocument();
  });

  it("speaks Bangla around the original texts", async () => {
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ locale: "bn" }));
    const user = userEvent.setup();
    renderDialog();
    expect(await screen.findByRole("dialog", { name: "তৃতীয় পক্ষের নোটিশ" })).toBeInTheDocument();
    expect(screen.getByText(/২টি উপাদান|2টি উপাদান/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /beta/ }));
    expect(screen.getByText("অসম্পূর্ণ লাইসেন্স তথ্য:")).toBeInTheDocument();
    // Technical notes and identifiers stay as published.
    expect(
      screen.getByText("version and license are not recorded in the binary").closest("[lang]"),
    ).toHaveAttribute("lang", "en");
  });
});

describe("about", () => {
  it("shows the version this build was made with, in English and Bangla", async () => {
    const { version } = (await import("../../src-tauri/tauri.conf.json")).default as {
      version: string;
    };
    const user = userEvent.setup();
    const view = render(
      <PreferencesProvider>
        <ServiceProvider
          client={new FakeServiceClient()}
          shell={new FakeShell()}
          pollIntervalMs={50}
        >
          <App />
        </ServiceProvider>
      </PreferencesProvider>,
    );
    await user.click(screen.getByRole("menuitem", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "About Downova" }));
    expect(screen.getByText(`Version ${version} — internal test build`)).toBeInTheDocument();
    view.unmount();
    localStorage.setItem("dm.ui.preferences.v1", JSON.stringify({ locale: "bn" }));
    render(
      <PreferencesProvider>
        <ServiceProvider
          client={new FakeServiceClient()}
          shell={new FakeShell()}
          pollIntervalMs={50}
        >
          <App />
        </ServiceProvider>
      </PreferencesProvider>,
    );
    await user.click(screen.getByRole("menuitem", { name: "সাহায্য" }));
    await user.click(screen.getByRole("menuitem", { name: "Downova সম্পর্কে" }));
    expect(
      screen.getByText(`সংস্করণ ${version} — অভ্যন্তরীণ পরীক্ষামূলক বিল্ড`),
    ).toBeInTheDocument();
  });
});
