import { useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "./preferences";
import { useDownloads, useServiceActions, useServiceState } from "../services/ServiceProvider";
import { toCallError } from "../services/service-client";
import { format, type MessageKey } from "../i18n/messages";
import { MenuBar, type Menu, type MenuEntry } from "../ui/MenuBar";
import { Dialog } from "../ui/Dialog";
import { Button } from "../ui/Button";
import { Banner } from "../ui/Banner";
import { DownloadsToolbar, type ToolbarCommand } from "../features/downloads/DownloadsToolbar";
import { CategoryTree } from "../features/downloads/CategoryTree";
import { DownloadTable } from "../features/downloads/DownloadTable";
import { AddUrlDialog, type PendingAdd } from "../features/downloads/AddUrlDialog";
import { DownloadDetails, type CommandProblem } from "../features/downloads/DownloadDetails";
import { canPause, canResume } from "../features/downloads/commands";
import { findCategory, matchesCategory, type CategoryId } from "../features/downloads/categories";
import { OptionsDialog } from "../features/settings/OptionsDialog";
import "./App.css";

type OpenDialog = "options" | "about" | "addUrl" | null;
type ItemExtra = Omit<Extract<MenuEntry, { kind: "item" }>, "kind" | "id" | "label">;

export function App() {
  const { t, theme, setTheme, locale, setLocale } = usePreferences();
  const service = useServiceState();
  const list = useDownloads();
  const actions = useServiceActions();
  const connected = service.status === "connected";
  const [category, setCategory] = useState<CategoryId>("all");
  const [showCategories, setShowCategories] = useState(true);
  const [dialog, setDialog] = useState<OpenDialog>(null);
  // Selection is by job ID: a row keeps it when the service renames the file (B1.6).
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<CommandProblem | null>(null);
  // The last add that was not confirmed: reopening Add URL offers the same link and key.
  const [pendingAdd, setPendingAdd] = useState<PendingAdd>(null);

  const downloads = list.items.filter((d) => matchesCategory(d, category));
  const selected = downloads.find((d) => d.job_id === selectedId);
  const categoryLabel = t(findCategory(category)?.labelKey ?? "categories.all");

  // Why service commands cannot run at all right now.
  const offline: string | undefined = connected
    ? undefined
    : service.status === "checking"
      ? t("unavailable.checking")
      : service.reason === "not_running"
        ? t("unavailable.disconnected")
        : service.reason === "incompatible_version"
          ? t("unavailable.incompatible")
          : t("unavailable.service");
  const notWired = offline ?? t("unavailable.notWired");

  function rowCommand(
    command: CommandProblem["command"],
    eligibility: (d: DownloadSnapshot | undefined) => MessageKey | null,
  ): ToolbarCommand {
    const reason = offline ?? (busy ? t("unavailable.busy") : null);
    if (reason) return { unavailableReason: reason };
    const why = eligibility(selected);
    if (why || !selected) return { unavailableReason: t(why ?? "unavailable.noSelection") };
    const jobId = selected.job_id;
    return {
      onClick: () => {
        setBusy(true);
        setProblem(null);
        (command === "pause" ? actions.pause(jobId) : actions.resume(jobId))
          .catch((thrown: unknown) => setProblem({ command, failure: toCallError(thrown, true) }))
          .finally(() => setBusy(false));
      },
    };
  }

  const add: ToolbarCommand = offline
    ? { unavailableReason: offline }
    : { onClick: () => setDialog("addUrl") };
  const resume = rowCommand("resume", canResume);
  const stop = rowCommand("pause", canPause);
  const removeReason = t("unavailable.remove");

  const command = (c: ToolbarCommand): ItemExtra =>
    c.unavailableReason !== undefined
      ? { unavailableReason: c.unavailableReason }
      : c.onClick
        ? { onSelect: c.onClick }
        : {};
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
        item("add", t("menu.tasks.addUrl"), command(add)),
        item("batch", t("menu.tasks.addBatch"), { unavailableReason: notWired }),
        item("import", t("menu.tasks.import"), { unavailableReason: notWired }),
        sep("s1"),
        item("exit", t("menu.tasks.exit"), { unavailableReason: notWired }),
      ],
    },
    {
      id: "file",
      label: t("menu.file"),
      entries: [
        item("resume", t("menu.file.resume"), command(resume)),
        item("stop", t("menu.file.stop"), command(stop)),
        item("redownload", t("menu.file.redownload"), { unavailableReason: notWired }),
        item("remove", t("menu.file.remove"), { unavailableReason: removeReason }),
        sep("s1"),
        item("properties", t("menu.file.properties"), { unavailableReason: notWired }),
      ],
    },
    {
      id: "downloads",
      label: t("menu.downloads"),
      entries: [
        item("stopAll", t("menu.downloads.stopAll"), { unavailableReason: notWired }),
        item("deleteCompleted", t("menu.downloads.deleteCompleted"), {
          unavailableReason: notWired,
        }),
        sep("s1"),
        item("scheduler", t("menu.downloads.scheduler"), { unavailableReason: notWired }),
        item("startQueue", t("menu.downloads.startQueue"), { unavailableReason: notWired }),
        item("stopQueue", t("menu.downloads.stopQueue"), { unavailableReason: notWired }),
        item("speedLimiter", t("menu.downloads.speedLimiter"), { unavailableReason: notWired }),
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

  // A desktop that lost (or never found) its service says so above the list and offers to
  // reconnect; the plain-browser build has no service bridge at all and keeps the empty state.
  const lost =
    service.status === "unavailable" && service.reason !== "not_implemented" ? service : null;

  return (
    <div className="app-window">
      <a className="skip-link" href="#download-list">
        {t("app.skipToContent")}
      </a>
      <header className="app-window__chrome">
        <MenuBar menus={menus} label={t("menu.label")} />
        <DownloadsToolbar
          add={add}
          resume={resume}
          stop={stop}
          deleteReason={removeReason}
          notWiredReason={notWired}
          onOptions={() => setDialog("options")}
        />
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
          {lost && (
            <div className="app-window__notice">
              <Banner
                tone="warning"
                title={
                  lost.reason === "incompatible_version"
                    ? t("service.incompatibleTitle")
                    : t("statusbar.disconnected")
                }
              >
                <p>{list.items.length > 0 ? t("service.staleBody") : t("service.downBody")}</p>
                {lost.reason === "not_running" && (
                  <Button onClick={actions.reconnect}>{t("service.reconnect")}</Button>
                )}
              </Banner>
            </div>
          )}
          <DownloadTable
            categoryLabel={categoryLabel}
            downloads={downloads}
            connected={connected}
            loading={connected && !list.loaded}
            selectedId={selectedId}
            onSelect={(id) => {
              setSelectedId(id);
              setProblem(null);
            }}
          />
          <DownloadDetails selected={selected} problem={problem} />
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

      {dialog === "addUrl" && (
        <AddUrlDialog
          unconfirmed={pendingAdd}
          onUnconfirmed={setPendingAdd}
          onClose={() => setDialog(null)}
          onAdded={(job) => {
            setDialog(null);
            setCategory("all");
            setSelectedId(job.job_id);
            setProblem(null);
          }}
        />
      )}
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
