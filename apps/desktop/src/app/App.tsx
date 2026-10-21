import { useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "./preferences";
import { useServiceState } from "../services/ServiceProvider";
import { format } from "../i18n/messages";
import { MenuBar, type Menu, type MenuEntry } from "../ui/MenuBar";
import { Dialog } from "../ui/Dialog";
import { Button } from "../ui/Button";
import { DownloadsToolbar } from "../features/downloads/DownloadsToolbar";
import { CategoryTree } from "../features/downloads/CategoryTree";
import { DownloadTable } from "../features/downloads/DownloadTable";
import { findCategory, matchesCategory, type CategoryId } from "../features/downloads/categories";
import { OptionsDialog } from "../features/settings/OptionsDialog";
import "./App.css";

type OpenDialog = "options" | "about" | null;
type ItemExtra = Omit<Extract<MenuEntry, { kind: "item" }>, "kind" | "id" | "label">;

// The v1 service has no list endpoint wired yet; the table renders whatever the service returns.
const NO_DOWNLOADS: DownloadSnapshot[] = [];

export function App() {
  const { t, theme, setTheme, locale, setLocale } = usePreferences();
  const service = useServiceState();
  const connected = service.status === "connected";
  const [category, setCategory] = useState<CategoryId>("all");
  const [showCategories, setShowCategories] = useState(true);
  const [dialog, setDialog] = useState<OpenDialog>(null);

  const downloads = NO_DOWNLOADS.filter((d) => matchesCategory(d, category));
  const categoryLabel = t(findCategory(category)?.labelKey ?? "categories.all");
  const needsService = connected ? undefined : t("unavailable.service");
  const needsSelection = connected ? t("unavailable.selection") : t("unavailable.service");
  const item = (id: string, label: string, extra: ItemExtra = {}): MenuEntry => ({
    kind: "item" as const,
    id,
    label,
    ...extra,
  });
  const sep = (id: string): MenuEntry => ({ kind: "separator" as const, id });

  const menus: Menu[] = [
    {
      id: "tasks",
      label: t("menu.tasks"),
      entries: [
        item("add", t("menu.tasks.addUrl"), { unavailableReason: needsService }),
        item("batch", t("menu.tasks.addBatch"), { unavailableReason: needsService }),
        item("import", t("menu.tasks.import"), { unavailableReason: needsService }),
        sep("s1"),
        item("exit", t("menu.tasks.exit"), { unavailableReason: needsService }),
      ],
    },
    {
      id: "file",
      label: t("menu.file"),
      entries: [
        item("resume", t("menu.file.resume"), { unavailableReason: needsSelection }),
        item("stop", t("menu.file.stop"), { unavailableReason: needsSelection }),
        item("redownload", t("menu.file.redownload"), { unavailableReason: needsSelection }),
        item("remove", t("menu.file.remove"), { unavailableReason: needsSelection }),
        sep("s1"),
        item("properties", t("menu.file.properties"), { unavailableReason: needsSelection }),
      ],
    },
    {
      id: "downloads",
      label: t("menu.downloads"),
      entries: [
        item("stopAll", t("menu.downloads.stopAll"), { unavailableReason: needsService }),
        item("deleteCompleted", t("menu.downloads.deleteCompleted"), {
          unavailableReason: needsService,
        }),
        sep("s1"),
        item("scheduler", t("menu.downloads.scheduler"), { unavailableReason: needsService }),
        item("startQueue", t("menu.downloads.startQueue"), { unavailableReason: needsService }),
        item("stopQueue", t("menu.downloads.stopQueue"), { unavailableReason: needsService }),
        item("speedLimiter", t("menu.downloads.speedLimiter"), { unavailableReason: needsService }),
        sep("s2"),
        item("options", t("menu.downloads.options"), { onSelect: () => setDialog("options") }),
      ],
    },
    {
      id: "view",
      label: t("menu.view"),
      entries: [
        item("categories", t("menu.view.categories"), {
          checked: showCategories,
          checkType: "checkbox",
          onSelect: () => setShowCategories((v) => !v),
        }),
        sep("s1"),
        item("system", t("menu.view.themeSystem"), {
          checked: theme === "system",
          onSelect: () => setTheme("system"),
        }),
        item("light", t("menu.view.themeLight"), {
          checked: theme === "light",
          onSelect: () => setTheme("light"),
        }),
        item("dark", t("menu.view.themeDark"), {
          checked: theme === "dark",
          onSelect: () => setTheme("dark"),
        }),
        sep("s2"),
        item("en", t("menu.view.langEn"), {
          checked: locale === "en",
          onSelect: () => setLocale("en"),
        }),
        item("bn", t("menu.view.langBn"), {
          checked: locale === "bn",
          onSelect: () => setLocale("bn"),
        }),
      ],
    },
    {
      id: "help",
      label: t("menu.help"),
      entries: [
        item("updates", t("menu.help.updates"), { unavailableReason: t("unavailable.updates") }),
        sep("s1"),
        item("about", t("menu.help.about"), { onSelect: () => setDialog("about") }),
      ],
    },
  ];

  const statusText =
    service.status === "checking"
      ? t("statusbar.checking")
      : connected
        ? t("statusbar.connected")
        : t("statusbar.disconnected");

  return (
    <div className="app-window">
      <a className="skip-link" href="#download-list">
        {t("app.skipToContent")}
      </a>
      <header className="app-window__chrome">
        <MenuBar menus={menus} label={t("menu.label")} />
        <DownloadsToolbar connected={connected} onOptions={() => setDialog("options")} />
      </header>

      <div
        className={`app-window__body${showCategories ? "" : " app-window__body--no-categories"}`}
      >
        {showCategories && (
          <CategoryTree
            selected={category}
            onSelect={setCategory}
            onClose={() => setShowCategories(false)}
          />
        )}
        <main id="download-list" className="app-window__main" tabIndex={-1}>
          <h1 className="visually-hidden">{t("app.name")}</h1>
          <DownloadTable
            categoryLabel={categoryLabel}
            downloads={downloads}
            connected={connected}
          />
        </main>
      </div>

      <footer className="statusbar" aria-label={t("statusbar.label")}>
        <span
          role="status"
          className={`statusbar__service statusbar__service--${connected ? "ok" : "off"}`}
        >
          {statusText}
        </span>
        <span className="statusbar__count">
          {format(t("statusbar.count"), { count: downloads.length })}
        </span>
      </footer>

      {dialog === "options" && <OptionsDialog onClose={() => setDialog(null)} />}
      {dialog === "about" && (
        <Dialog
          title={t("about.title")}
          closeLabel={t("options.close")}
          onClose={() => setDialog(null)}
          footer={
            <Button variant="primary" onClick={() => setDialog(null)}>
              {t("options.done")}
            </Button>
          }
        >
          <p className="about__name">{t("app.name")}</p>
          <p className="about__version">{t("app.version")}</p>
          <p>{t("about.body")}</p>
        </Dialog>
      )}
    </div>
  );
}
