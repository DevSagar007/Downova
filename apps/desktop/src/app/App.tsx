import { useEffect, useRef, useState } from "react";
import type { DownloadSnapshot, MoveTarget, QueueInfo } from "@dm/contracts";
import { usePreferences } from "./preferences";
import {
  useDownloads,
  useServiceActions,
  useServiceState,
  useShell,
} from "../services/ServiceProvider";
import { ExitDialog } from "../features/lifecycle/ExitDialog";
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
import { canCancel, canPause, canRemove, canResume } from "../features/downloads/commands";
import {
  CANCEL_COPY,
  ConfirmCommandDialog,
  REMOVE_COPY,
} from "../features/downloads/ConfirmCommandDialog";
import {
  byQueuePosition,
  categoryTree,
  findCategory,
  MAIN_QUEUE,
  matchesCategory,
  queueOf,
  queueOfNode,
  type CategoryId,
} from "../features/downloads/categories";
import { ManageQueuesDialog } from "../features/queues/ManageQueuesDialog";
import { JobQueueDialog } from "../features/queues/JobQueueDialog";
import { matchesSearch } from "../features/downloads/search";
import { isFinished, type ClearOutcome, type ClearTarget } from "../features/downloads/clear";
import { DownloadSearch } from "../features/downloads/DownloadSearch";
import { ClearFinishedDialog } from "../features/downloads/ClearFinishedDialog";
import { OptionsDialog } from "../features/settings/OptionsDialog";
import "./App.css";

type OpenDialog =
  | "options"
  | "about"
  | "addUrl"
  | "exit"
  | "cancel"
  | "remove"
  | "clear"
  | "queues"
  | "jobQueue"
  | null;
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
  // History search is local presentation state; it never changes the service's list (CORE-10).
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  // The frozen scope of a Clear finished confirmation (jobs that were finished when it opened).
  const [clearTargets, setClearTargets] = useState<ClearTarget[]>([]);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<CommandProblem | null>(null);
  // Where focus goes once a closing dialog has handed it back to its opener (a closed menu).
  const focusAfterDialog = useRef<string | null>(null);
  useEffect(() => {
    const wanted = focusAfterDialog.current;
    if (dialog !== null || wanted === null) return;
    focusAfterDialog.current = null;
    const row = document.querySelector<HTMLElement>(`[data-job-id="${CSS.escape(wanted)}"]`);
    (row ?? document.getElementById("download-list"))?.focus();
  }, [dialog]);
  // The job a Cancel or Remove confirmation is about (fixed when the dialog opens).
  const [target, setTarget] = useState<DownloadSnapshot | null>(null);
  // Polite announcements of meaningful changes only (design.md §10): never per-poll progress.
  const [removedNote, setRemovedNote] = useState<{ text: string; seq: number } | null>(null);
  const ended = list.ended;
  const announcement =
    removedNote && (!ended || removedNote.seq > ended.seq)
      ? removedNote.text
      : ended
        ? format(t(`announce.${ended.phase}`), { name: ended.name })
        : "";
  // The last add that was not confirmed: reopening Add URL offers the same link and key.
  const [pendingAdd, setPendingAdd] = useState<PendingAdd>(null);
  const shell = useShell();
  const exitLabel = t("menu.tasks.exit");
  const openLabel = t("tray.open");

  // The tray speaks the window's language; its "Exit and stop downloads" opens the same dialog.
  useEffect(() => {
    if (shell.available) void shell.setTrayLabels(openLabel, exitLabel);
  }, [shell, openLabel, exitLabel]);
  useEffect(() => shell.onExitRequested(() => setDialog("exit")), [shell]);

  const shownQueue = queueOfNode(category);
  const view: CategoryId =
    shownQueue !== null &&
    list.queues.length > 0 &&
    !list.queues.some((q) => q.queue_id === shownQueue)
      ? "all"
      : category;
  const viewQueue = queueOfNode(view);
  const inView = list.items.filter((d) => matchesCategory(d, view));
  // Inside a queue the rows follow its execution order (B2.11); column sorting stays a view.
  const categoryDownloads = viewQueue === null ? inView : [...inView].sort(byQueuePosition);
  const downloads = categoryDownloads.filter((d) => matchesSearch(d, query));
  const selected = downloads.find((d) => d.job_id === selectedId);
  const viewNode = findCategory(view, categoryTree(list.queues));
  const categoryLabel = viewNode?.labelKey
    ? t(viewNode.labelKey)
    : (viewNode?.label ?? t("categories.all"));
  const searching = query.trim() !== "";
  // Clear finished covers every known finished job, whatever the category and search show.
  const clearable = list.items.filter(isFinished);

  // The match count is announced after typing settles; never on every keystroke (design.md §10).
  const [searchNote, setSearchNote] = useState<{ query: string; text: string } | null>(null);
  useEffect(() => {
    if (!searching) return;
    const timer = setTimeout(() => {
      setSearchNote({
        query,
        text:
          downloads.length === 0
            ? t("search.none")
            : format(t("search.matchCount"), { count: downloads.length }),
      });
    }, 300);
    return () => clearTimeout(timer);
  }, [searching, query, downloads.length, t]);

  // Ctrl/Cmd+F focuses the search field, except while a dialog owns the keyboard.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (dialog !== null || event.defaultPrevented) return;
      if (
        (event.ctrlKey || event.metaKey) &&
        !event.altKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "f"
      ) {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialog]);

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
  const deleteReason = t("unavailable.remove");

  // Cancel and Remove ask first; the dialog sends the request.
  function confirmCommand(
    kind: "cancel" | "remove",
    eligibility: (d: DownloadSnapshot | undefined) => MessageKey | null,
  ): ToolbarCommand {
    const reason = offline ?? (busy ? t("unavailable.busy") : null);
    if (reason) return { unavailableReason: reason };
    const why = eligibility(selected);
    if (why || !selected) return { unavailableReason: t(why ?? "unavailable.noSelection") };
    const job = selected;
    return {
      onClick: () => {
        setProblem(null);
        setTarget(job);
        setDialog(kind);
      },
    };
  }
  // The queue Start/Stop act on: the selected queue node, else the selected download's queue,
  // else the main queue.
  const unfinished = (d: DownloadSnapshot | undefined) =>
    d !== undefined && !["completed", "failed", "cancelled"].includes(d.phase);
  const targetQueueId =
    viewQueue ?? (unfinished(selected) && selected ? queueOf(selected) : MAIN_QUEUE);
  const targetQueue: QueueInfo | undefined = list.queues.find((q) => q.queue_id === targetQueueId);
  const queueName = (q: QueueInfo) =>
    q.queue_id === MAIN_QUEUE ? t("categories.mainQueue") : q.name;
  function queueCommand(start: boolean): ToolbarCommand {
    const reason = offline ?? (busy ? t("unavailable.busy") : null);
    if (reason) return { unavailableReason: reason };
    if (!targetQueue) return { unavailableReason: notWired };
    const name = queueName(targetQueue);
    if (targetQueue.running === start) {
      return {
        unavailableReason: format(
          t(start ? "unavailable.queueRunning" : "unavailable.queueStopped"),
          { name },
        ),
      };
    }
    const id = targetQueue.queue_id;
    return {
      onClick: () => {
        setBusy(true);
        setProblem(null);
        (start ? actions.startQueue(id) : actions.stopQueue(id))
          .then(() =>
            setRemovedNote({
              text: format(t(start ? "announce.queueStarted" : "announce.queueStopped"), { name }),
              seq: (list.ended?.seq ?? 0) + 1,
            }),
          )
          .catch(() => {})
          .finally(() => setBusy(false));
      },
    };
  }
  const startQueue = queueCommand(true);
  const stopQueue = queueCommand(false);

  /** Commands on the selected download's place in its queue (keyboard: menus, Alt+arrows). */
  function queuedCommand(run: (job: DownloadSnapshot) => void): ToolbarCommand {
    const reason = offline ?? (busy ? t("unavailable.busy") : null);
    if (reason) return { unavailableReason: reason };
    if (!selected) return { unavailableReason: t("unavailable.noSelection") };
    if (!unfinished(selected)) return { unavailableReason: t("unavailable.notQueued") };
    const job = selected;
    return { onClick: () => run(job) };
  }
  function moveCommand(to: MoveTarget): ToolbarCommand {
    return queuedCommand((job) => {
      setBusy(true);
      setProblem(null);
      actions
        .moveJob(job.job_id, to)
        .then(() =>
          setRemovedNote({
            text: format(t("announce.moved"), { name: job.display_name }),
            seq: (list.ended?.seq ?? 0) + 1,
          }),
        )
        .catch(() => {})
        .finally(() => setBusy(false));
    });
  }
  const moveUp = moveCommand("up");
  const moveDown = moveCommand("down");
  const moveTop = moveCommand("top");
  const moveBottom = moveCommand("bottom");
  const jobQueue = queuedCommand(() => setDialog("jobQueue"));

  // Alt+Up / Alt+Down move the selected download inside its queue (outside dialogs).
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (dialog !== null || event.defaultPrevented || !event.altKey) return;
      const command =
        event.key === "ArrowUp" ? moveUp : event.key === "ArrowDown" ? moveDown : null;
      if (!command?.onClick) return;
      event.preventDefault();
      command.onClick();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const cancel = confirmCommand("cancel", canCancel);
  const remove = confirmCommand("remove", canRemove);

  const clearReason = offline ?? (busy ? t("unavailable.busy") : null);
  const clear = clearReason
    ? { unavailableReason: clearReason }
    : clearable.length === 0
      ? { unavailableReason: t("clear.none") }
      : {
          onSelect: () => {
            setProblem(null);
            // Freeze the scope: jobs finishing later are not added to this confirmation.
            setClearTargets(clearable.map((d) => ({ jobId: d.job_id, name: d.display_name })));
            setDialog("clear");
          },
        };

  /** After a bulk clear: keep a surviving selection, or move to the first remaining row. */
  function afterClear(outcome: ClearOutcome) {
    const removed = new Set(outcome.succeeded);
    const remaining = downloads.filter((d) => !removed.has(d.job_id));
    setSelectedId((prev) =>
      prev !== null && !removed.has(prev) ? prev : (remaining[0]?.job_id ?? null),
    );
    if (outcome.failed.length > 0) return;
    setDialog(null);
    setClearTargets([]);
    setRemovedNote({
      text: format(t("clear.announce"), { count: outcome.succeeded.length }),
      seq: (list.ended?.seq ?? 0) + 1,
    });
    focusAfterDialog.current = remaining[0]?.job_id ?? "list";
  }

  /** After a removal: select the next row (or the previous one) and move focus there. */
  function afterRemove(job: DownloadSnapshot) {
    const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-job-id]"));
    const at = buttons.findIndex((b) => b.dataset.jobId === job.job_id);
    const next = buttons[at + 1] ?? buttons[at - 1] ?? null;
    setSelectedId(next?.dataset.jobId ?? null);
    setDialog(null);
    setTarget(null);
    setRemovedNote({
      text: format(t("announce.removed"), { name: job.display_name }),
      seq: (list.ended?.seq ?? 0) + 1,
    });
    focusAfterDialog.current = next?.dataset.jobId ?? "list";
  }

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
        item(
          "exit",
          exitLabel,
          shell.available
            ? { onSelect: () => setDialog("exit") }
            : { unavailableReason: t("unavailable.desktopOnly") },
        ),
      ],
    },
    {
      id: "file",
      label: t("menu.file"),
      entries: [
        item("resume", t("menu.file.resume"), command(resume)),
        item("stop", t("menu.file.stop"), command(stop)),
        item("cancel", t("menu.file.cancel"), command(cancel)),
        item("redownload", t("menu.file.redownload"), { unavailableReason: notWired }),
        item("remove", t("menu.file.remove"), command(remove)),
        sep("s0"),
        item("queue", t("menu.file.queue"), command(jobQueue)),
        item("moveTop", t("menu.file.moveTop"), command(moveTop)),
        item("moveUp", t("menu.file.moveUp"), command(moveUp)),
        item("moveDown", t("menu.file.moveDown"), command(moveDown)),
        item("moveBottom", t("menu.file.moveBottom"), command(moveBottom)),
        item("clearFinished", t("menu.file.clearFinished"), clear),
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
        item("startQueue", t("menu.downloads.startQueue"), command(startQueue)),
        item("stopQueue", t("menu.downloads.stopQueue"), command(stopQueue)),
        item(
          "manageQueues",
          t("menu.downloads.manageQueues"),
          offline ? { unavailableReason: offline } : { onSelect: () => setDialog("queues") },
        ),
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
          startQueue={startQueue}
          stopQueue={stopQueue}
          deleteReason={deleteReason}
          notWiredReason={notWired}
          onOptions={() => setDialog("options")}
        />
      </header>

      <div
        className={`app-window__body${showCategories ? "" : " app-window__body--no-categories"}`}
      >
        {showCategories && (
          <CategoryTree
            selected={view}
            onSelect={setCategory}
            queues={list.queues}
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
                {list.reconnecting && <p role="status">{t("service.reconnecting")}</p>}
                {lost.reason === "not_running" && !list.reconnecting && (
                  <Button onClick={actions.reconnect}>{t("service.reconnect")}</Button>
                )}
              </Banner>
            </div>
          )}
          <DownloadSearch
            value={query}
            onChange={(value) => {
              setQuery(value);
              setSearchNote(null);
            }}
            inputRef={searchRef}
            resultCount={downloads.length}
          />
          <DownloadTable
            categoryLabel={categoryLabel}
            downloads={downloads}
            connected={connected}
            loading={connected && !list.loaded}
            searchActive={searching}
            categoryCount={categoryDownloads.length}
            selectedId={selectedId}
            onSelect={(id) => {
              setSelectedId(id);
              setProblem(null);
            }}
          />
          <DownloadDetails selected={selected} problem={problem} onCancel={cancel.onClick} />
        </main>
      </div>

      <footer className="statusbar" aria-label={t("statusbar.label")}>
        <span
          role="status"
          className={`statusbar__service statusbar__service--${connected ? "ok" : "off"}`}
        >
          {statusText}
        </span>
        <span className="visually-hidden" role="status" aria-live="polite">
          {announcement}
        </span>
        <span className="visually-hidden" role="status" aria-live="polite">
          {searching && searchNote?.query === query ? searchNote.text : ""}
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
          onShowExisting={(jobId) => {
            setDialog(null);
            setCategory("all");
            setQuery("");
            setSelectedId(jobId);
            setProblem(null);
            focusAfterDialog.current = jobId;
          }}
        />
      )}
      {dialog === "exit" && <ExitDialog onClose={() => setDialog(null)} />}
      {dialog === "cancel" && target && (
        <ConfirmCommandDialog
          name={target.display_name}
          copy={CANCEL_COPY}
          run={() => actions.cancel(target.job_id)}
          onDone={() => {
            setDialog(null);
            setTarget(null);
          }}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "remove" && target && (
        <ConfirmCommandDialog
          name={target.display_name}
          copy={REMOVE_COPY}
          run={() => actions.remove(target.job_id)}
          onDone={afterRemove}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "clear" && (
        <ClearFinishedDialog
          targets={clearTargets}
          remove={(jobId) => actions.remove(jobId)}
          onCompleted={afterClear}
          onClose={() => {
            setDialog(null);
            setClearTargets([]);
          }}
        />
      )}
      {dialog === "options" && <OptionsDialog onClose={() => setDialog(null)} />}
      {dialog === "queues" && <ManageQueuesDialog onClose={() => setDialog(null)} />}
      {dialog === "jobQueue" && selected && (
        <JobQueueDialog job={selected} onClose={() => setDialog(null)} />
      )}
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
