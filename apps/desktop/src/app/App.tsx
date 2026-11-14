import { useEffect, useId, useMemo, useRef, useState, type DragEvent } from "react";
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
import { BatchAddDialog } from "../features/downloads/BatchAddDialog";
import { ClipboardPrompt } from "../features/downloads/ClipboardPrompt";
import { FinishActionDialog, FinishCountdown } from "../features/downloads/FinishActionDialog";
import { COUNTDOWN_SECONDS, pendingWork } from "../features/downloads/finish";
import type { PowerAction } from "../services/service-client";
import {
  acceptsDrag,
  clipboardOffers,
  readDrop,
  type MonitorMemory,
} from "../features/downloads/intake";
import { PropertiesDialog } from "../features/downloads/PropertiesDialog";
import { MAX_LIST_BYTES } from "../features/downloads/batch";
import {
  DownloadDetails,
  type CommandProblem,
  type DetailsNotice,
} from "../features/downloads/DownloadDetails";
import {
  canCancel,
  canOpenFolder,
  canPause,
  canRefreshLink,
  canRemove,
  canResume,
  canRetry,
} from "../features/downloads/commands";
import { RetryDialog } from "../features/downloads/RetryDialog";
import { RefreshLinkDialog } from "../features/downloads/RefreshLinkDialog";
import {
  CANCEL_COPY,
  ConfirmCommandDialog,
  REMOVE_COPY,
  REMOVE_FILE_COPY,
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
import { DiagnosticsDialog } from "../features/support/DiagnosticsDialog";
import { BrowserConnectionsDialog } from "../features/support/BrowserConnectionsDialog";
import { UpdateDialog, readAutoCheck } from "../features/support/UpdateDialog";
import { NoticesDialog } from "../features/support/NoticesDialog";
import "./App.css";

/** F02: how often the clipboard is read while the monitor is on. */
export const CLIPBOARD_POLL_MS = 2_000;

type OpenDialog =
  | "options"
  | "about"
  | "diagnostics"
  | "browsers"
  | "updates"
  | "notices"
  | "addUrl"
  | "exit"
  | "cancel"
  | "remove"
  | "clear"
  | "queues"
  | "jobQueue"
  | "retry"
  | "refresh"
  | "batch"
  | "properties"
  | "finish"
  | null;
type ItemExtra = Omit<Extract<MenuEntry, { kind: "item" }>, "kind" | "id" | "label">;

export function App() {
  const { t, theme, setTheme, locale, setLocale, clipboardMonitor, ignoredHosts } =
    usePreferences();
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
  // The outcome of the last Open containing folder, until the selection changes (B2.19).
  const [notice, setNotice] = useState<DetailsNotice | null>(null);
  // Where focus goes once a closing dialog has handed it back to its opener (a closed menu).
  const focusAfterDialog = useRef<string | null>(null);
  // Manage queues was opened from Options (B2.31): closing it returns there.
  const queuesFromOptions = useRef(false);
  // The table's rows in visual order (sorting applied), kept by the table.
  const tableOrder = useRef<readonly DownloadSnapshot[]>([]);
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
  // B4: a link Add URL opens with (dropped or copied), and the intake state.
  const [addUrl, setAddUrl] = useState<string | undefined>(undefined);
  const [dropping, setDropping] = useState(false);
  const [dropUnsupported, setDropUnsupported] = useState(false);
  const [copied, setCopied] = useState<string[]>([]);
  // F08: the finish action chosen for this run, the countdown's seconds left, and a failed run.
  const [finish, setFinish] = useState<PowerAction | null>(null);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [finishFailed, setFinishFailed] = useState(false);
  // Add batch download / Import download list: the text the dialog starts with, and why it is
  // empty or partial (both chosen before the dialog opens).
  const [batch, setBatch] = useState<{ text: string; note: MessageKey | null }>({
    text: "",
    note: null,
  });
  const listFileId = useId();

  /**
   * Reads the clipboard through the desktop on the user's command (the webview itself may not);
   * a failure or no text opens the dialog empty with the reason.
   */
  async function openBatchFromClipboard() {
    let text = "";
    let note: MessageKey | null = null;
    try {
      text = (await shell.clipboardText()) ?? "";
      if (text.trim() === "") note = "batch.clipboardEmpty";
    } catch {
      note = "batch.clipboardDenied";
    }
    setBatch({ text, note });
    setDialog("batch");
  }

  /** Import download list: the system file chooser (an event handler, not read during render). */
  function chooseListFile() {
    (document.getElementById(listFileId) as HTMLInputElement | null)?.click();
  }

  /** A chosen list file (text, at most MAX_LIST_BYTES) opens the dialog with its contents. */
  async function openBatchFromFile(file: File | undefined) {
    if (!file) return;
    let text = "";
    let note: MessageKey | null = null;
    if (file.size > MAX_LIST_BYTES) note = "batch.fileTooLarge";
    else {
      try {
        text = await file.text();
      } catch {
        note = "batch.fileUnreadable";
      }
    }
    setBatch({ text, note });
    setDialog("batch");
  }
  const shell = useShell();
  const exitLabel = t("menu.tasks.exit");
  const openLabel = t("tray.open");

  // CORE-09: notifications are shown by the service in the window's language. When the language
  // changes while they are on, the service is told (no notification is shown for this).
  const { getNotifications, setNotifications } = actions;
  useEffect(() => {
    if (!connected) return;
    let active = true;
    getNotifications()
      .then((s) => {
        if (active && s.enabled && s.locale !== locale) void setNotifications(true, locale);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [connected, locale, getNotifications, setNotifications]);

  // The tray speaks the window's language; its "Exit and stop downloads" opens the same dialog.
  useEffect(() => {
    if (shell.available) void shell.setTrayLabels(openLabel, exitLabel);
  }, [shell, openLabel, exitLabel]);
  useEffect(() => shell.onExitRequested(() => setDialog("exit")), [shell]);
  // ADV09: an automatic check only when the user opted in, once per start. It only checks; a
  // download, preparation or installation always needs the user's action in the Updates dialog.
  useEffect(() => {
    if (shell.available && readAutoCheck()) void shell.updateCheck().catch(() => undefined);
  }, [shell]);

  const shownQueue = queueOfNode(category);
  const view: CategoryId =
    shownQueue !== null &&
    list.queues.length > 0 &&
    !list.queues.some((q) => q.queue_id === shownQueue)
      ? "all"
      : category;
  const viewQueue = queueOfNode(view);
  // Whole-list work runs only when the list, the category or the search changes (ADV12).
  const inView = useMemo(
    () => list.items.filter((d) => matchesCategory(d, view)),
    [list.items, view],
  );
  // Inside a queue the rows follow its execution order (B2.11); column sorting stays a view.
  const categoryDownloads = useMemo(
    () => (viewQueue === null ? inView : [...inView].sort(byQueuePosition)),
    [inView, viewQueue],
  );
  const downloads = useMemo(
    () => categoryDownloads.filter((d) => matchesSearch(d, query)),
    [categoryDownloads, query],
  );
  const selected = downloads.find((d) => d.job_id === selectedId);
  const viewNode = findCategory(view, categoryTree(list.queues));
  const categoryLabel = viewNode?.labelKey
    ? t(viewNode.labelKey)
    : (viewNode?.label ?? t("categories.all"));
  const searching = query.trim() !== "";
  // Clear finished covers every known finished job, whatever the category and search show.
  const clearable = useMemo(() => list.items.filter(isFinished), [list.items]);

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
          : service.reason === "list_unreadable"
            ? t("unavailable.listUnreadable")
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
    : {
        onClick: () => {
          setAddUrl(undefined);
          setDialog("addUrl");
        },
      };

  // ---------------------------------------------------------------- F08 finish action
  // Armed for this run only. Once nothing the user wants running is left (queued, transferring,
  // verifying, scanning and finalizing all count), the window comes forward and counts down; any
  // download that starts again stops the countdown (the choice stays). At zero the desktop runs the
  // system command; afterwards (or on Cancel) nothing is remembered.
  const pending = pendingWork(list.items);
  const ready = finish !== null && list.loaded && pending === 0;
  const [prevReady, setPrevReady] = useState(false);
  if (ready !== prevReady) {
    setPrevReady(ready);
    setCountdown(ready ? COUNTDOWN_SECONDS : null);
    if (ready) void shell.showWindow();
  }
  useEffect(() => {
    if (countdown === null || finish === null) return;
    const action = finish;
    const timer = setTimeout(() => {
      if (countdown > 1) {
        setCountdown(countdown - 1);
        return;
      }
      // The last second passed without Cancel: run once, then forget the choice.
      setFinish(null);
      setCountdown(null);
      void shell.powerRun(action).then((ok) => setFinishFailed(!ok));
    }, 1000);
    return () => clearTimeout(timer);
  }, [countdown, finish, shell]);

  // ---------------------------------------------------------------- B4 intake (F02, F03)
  // F02: while the monitor is on and the service is reachable, the desktop reads the clipboard
  // every CLIPBOARD_POLL_MS. The text present when monitoring starts is only noted, never offered;
  // later copies offer new file links (see clipboardOffers) as a prompt. Nothing is kept or sent.
  const monitorOn = clipboardMonitor && !offline && shell.available;
  // The latest ignore list, read by the polling loop without restarting it.
  const ignoredRef = useRef(ignoredHosts);
  useEffect(() => {
    ignoredRef.current = ignoredHosts;
  }, [ignoredHosts]);
  useEffect(() => {
    if (!monitorOn) return;
    let active = true;
    let started = false;
    const memory: MonitorMemory = { lastText: null, offered: new Set() };
    const tick = async () => {
      let text: string | null = null;
      try {
        text = await shell.clipboardText();
      } catch {
        return;
      }
      if (!active) return;
      if (!started) {
        started = true;
        memory.lastText = text;
        return;
      }
      const offers = clipboardOffers(text, memory, ignoredRef.current);
      if (offers.length > 0)
        setCopied((prev) => [...prev, ...offers.filter((l) => !prev.includes(l))]);
    };
    void tick();
    const timer = setInterval(() => void tick(), CLIPBOARD_POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [monitorOn, shell]);

  // F03: links dropped onto the window prefill Add URL (one) or the batch dialog (several, or a
  // list file); anything else offers paste or import. Nothing is added before the user chooses.
  function openWithLinks(links: string[]) {
    if (links.length === 1) {
      setAddUrl(links[0]);
      setDialog("addUrl");
    } else {
      setBatch({ text: links.join("\n"), note: null });
      setDialog("batch");
    }
  }
  const dropHandlers = {
    onDragOver: (e: DragEvent) => {
      if (!acceptsDrag(Array.from(e.dataTransfer.types))) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = offline || dialog ? "none" : "copy";
      if (!offline && !dialog) setDropping(true);
    },
    onDragLeave: (e: DragEvent) => {
      if (e.relatedTarget === null) setDropping(false);
    },
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      setDropping(false);
      if (offline || dialog) return;
      const dropped = readDrop(e.dataTransfer);
      if (dropped.kind === "links") openWithLinks(dropped.links);
      else if (dropped.kind === "listFile") void openBatchFromFile(dropped.file);
      else setDropUnsupported(true);
    },
  };
  const resume = rowCommand("resume", canResume);
  const stop = rowCommand("pause", canPause);
  // Stop all (IDM parity, 2026-11-14): a manual pause of every download that can be paused,
  // through the same pause the single Stop sends. Nothing else is touched.
  const pausable = list.items.filter((d) => canPause(d) === null);
  const stopAll: ToolbarCommand =
    offline || busy
      ? { unavailableReason: offline ?? t("unavailable.busy") }
      : pausable.length === 0
        ? { unavailableReason: t("unavailable.nothingToStop") }
        : {
            onClick: () => {
              setBusy(true);
              setProblem(null);
              const ids = pausable.map((d) => d.job_id);
              void Promise.allSettled(ids.map((id) => actions.pause(id)))
                .then((results) => {
                  const failed = results.find((r) => r.status === "rejected");
                  if (failed)
                    setProblem({ command: "pause", failure: toCallError(failed.reason, true) });
                })
                .finally(() => setBusy(false));
            },
          };
  // Scheduler opens Manage queues, where each queue's schedule is set (CORE-08).
  const scheduler: ToolbarCommand = offline
    ? { unavailableReason: offline }
    : { onClick: () => setDialog("queues") };

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

  /** Retry and Refresh link open their own dialog for the selected job (B2.16/B2.17). */
  function dialogCommand(
    kind: "retry" | "refresh",
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
        setNotice(null);
        setTarget(job);
        setDialog(kind);
      },
    };
  }
  const retry = dialogCommand("retry", canRetry);
  const refreshLink = dialogCommand("refresh", canRefreshLink);

  // Open containing folder (B2.19): only on this explicit action; the shell asks the service
  // where the job's file is and only ever reveals it.
  const openFolderReason = offline ?? (busy ? t("unavailable.busy") : null);
  const openFolderWhy = canOpenFolder(selected);
  const openFolder: ToolbarCommand =
    openFolderReason || openFolderWhy || !selected
      ? { unavailableReason: openFolderReason ?? t(openFolderWhy ?? "unavailable.noSelection") }
      : {
          onClick: () => {
            const jobId = selected.job_id;
            setNotice(null);
            actions
              .reveal(jobId)
              .then((how) => {
                if (how === "folder_file_missing") {
                  setNotice({
                    tone: "warning",
                    title: t("openFolder.fileMissing"),
                    lines: [],
                  });
                }
              })
              .catch((thrown: unknown) => {
                const failure = toCallError(thrown);
                const lines =
                  failure.outcome === "rejected"
                    ? [
                        t(`error.kind.${failure.error.kind}`),
                        ...(failure.error.next_action === "none"
                          ? []
                          : [t(`error.action.${failure.error.next_action}`)]),
                      ]
                    : [t("command.unavailable")];
                setNotice({ tone: "danger", title: t("openFolder.failed"), lines });
              });
          },
        };

  /** After Retry or Refresh link: keep the row selected and announce the outcome once. */
  function afterReopen(job: DownloadSnapshot, key: MessageKey) {
    setDialog(null);
    setTarget(null);
    setSelectedId(job.job_id);
    setRemovedNote({
      text: format(t(key), { name: job.display_name }),
      seq: (list.ended?.seq ?? 0) + 1,
    });
    focusAfterDialog.current = job.job_id;
  }

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

  /** After a removal: select the next row (or the previous one) and move focus there. The
   * neighbour comes from the table's full visual order: with a virtualized list it may not be
   * rendered (a selected row scrolled out of view is rendered alone), and selecting it pins it. */
  /** Whether the last confirmed Remove also deleted the file (for the announcement). */
  const removedWithFile = useRef(false);

  function afterRemove(job: DownloadSnapshot) {
    const order = tableOrder.current;
    const at = order.findIndex((d) => d.job_id === job.job_id);
    const next = at < 0 ? null : (order[at + 1] ?? order[at - 1] ?? null);
    setSelectedId(next?.job_id ?? null);
    setDialog(null);
    setTarget(null);
    setRemovedNote({
      text: format(t(removedWithFile.current ? "announce.removedDeleted" : "announce.removed"), {
        name: job.display_name,
      }),
      seq: (list.ended?.seq ?? 0) + 1,
    });
    focusAfterDialog.current = next?.job_id ?? "list";
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
        item(
          "batch",
          t("menu.tasks.addBatch"),
          offline
            ? { unavailableReason: offline }
            : { onSelect: () => void openBatchFromClipboard() },
        ),
        item(
          "import",
          t("menu.tasks.import"),
          offline ? { unavailableReason: offline } : { onSelect: chooseListFile },
        ),
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
        item("redownload", t("menu.file.redownload"), command(retry)),
        item("refreshLink", t("menu.file.refreshLink"), command(refreshLink)),
        item("openFolder", t("menu.file.openFolder"), command(openFolder)),
        item("remove", t("menu.file.remove"), command(remove)),
        sep("s0"),
        item("queue", t("menu.file.queue"), command(jobQueue)),
        item("moveTop", t("menu.file.moveTop"), command(moveTop)),
        item("moveUp", t("menu.file.moveUp"), command(moveUp)),
        item("moveDown", t("menu.file.moveDown"), command(moveDown)),
        item("moveBottom", t("menu.file.moveBottom"), command(moveBottom)),
        item("clearFinished", t("menu.file.clearFinished"), clear),
        sep("s1"),
        item(
          "properties",
          t("menu.file.properties"),
          selected
            ? { onSelect: () => setDialog("properties") }
            : { unavailableReason: t("unavailable.noSelection") },
        ),
      ],
    },
    {
      id: "downloads",
      label: t("menu.downloads"),
      entries: [
        item("stopAll", t("menu.downloads.stopAll"), command(stopAll)),
        item("deleteCompleted", t("menu.downloads.deleteCompleted"), clear),
        sep("s1"),
        item("scheduler", t("menu.downloads.scheduler"), command(scheduler)),
        item("startQueue", t("menu.downloads.startQueue"), command(startQueue)),
        item("stopQueue", t("menu.downloads.stopQueue"), command(stopQueue)),
        item(
          "manageQueues",
          t("menu.downloads.manageQueues"),
          offline ? { unavailableReason: offline } : { onSelect: () => setDialog("queues") },
        ),
        item("speedLimiter", t("menu.downloads.speedLimiter"), {
          onSelect: () => setDialog("options"),
        }),
        item(
          "finish",
          t("menu.downloads.finish"),
          offline ? { unavailableReason: offline } : { onSelect: () => setDialog("finish") },
        ),
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
        item("updates", t("menu.help.updates"), { onSelect: () => setDialog("updates") }),
        item("diagnostics", t("menu.help.diagnostics"), {
          onSelect: () => setDialog("diagnostics"),
        }),
        item("browsers", t("menu.help.browsers"), { onSelect: () => setDialog("browsers") }),
        item("notices", t("menu.help.notices"), { onSelect: () => setDialog("notices") }),
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
    <div className="app-window" {...dropHandlers}>
      {dropping && (
        <div className="app-window__drop" aria-hidden="true">
          <p>{t("drop.hint")}</p>
        </div>
      )}
      <a className="skip-link" href="#download-list">
        {t("app.skipToContent")}
      </a>
      <header className="app-window__chrome">
        <MenuBar menus={menus} label={t("menu.label")} />
        <input
          id={listFileId}
          type="file"
          accept=".txt,.lst,.csv,text/plain"
          hidden
          aria-hidden="true"
          tabIndex={-1}
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            void openBatchFromFile(file);
          }}
        />
        <DownloadsToolbar
          add={add}
          resume={resume}
          stop={stop}
          startQueue={startQueue}
          stopQueue={stopQueue}
          deleteCommand={remove}
          stopAll={stopAll}
          deleteCompleted={
            "onSelect" in clear
              ? { onClick: clear.onSelect }
              : { unavailableReason: clear.unavailableReason }
          }
          scheduler={scheduler}
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
                    : lost.reason === "list_unreadable"
                      ? t("service.listUnreadableTitle")
                      : t("statusbar.disconnected")
                }
              >
                <p>
                  {lost.reason === "list_unreadable"
                    ? t("service.listUnreadableBody")
                    : list.items.length > 0
                      ? t("service.staleBody")
                      : t("service.downBody")}
                </p>
                {list.reconnecting && <p role="status">{t("service.reconnecting")}</p>}
                {(lost.reason === "not_running" || lost.reason === "list_unreadable") &&
                  !list.reconnecting && (
                    <Button onClick={actions.reconnect}>{t("service.reconnect")}</Button>
                  )}
              </Banner>
            </div>
          )}
          {finish !== null && countdown === null && (
            <div className="app-window__notice">
              <Banner tone="info" title={t(`finish.armed.${finish}`)}>
                <div className="app-window__notice-actions">
                  <Button onClick={() => setFinish(null)}>{t("finish.cancel")}</Button>
                </div>
              </Banner>
            </div>
          )}
          {finishFailed && (
            <div className="app-window__notice">
              <Banner tone="danger" title={t("finish.failed")}>
                <div className="app-window__notice-actions">
                  <Button onClick={() => setFinishFailed(false)}>{t("drop.dismiss")}</Button>
                </div>
              </Banner>
            </div>
          )}
          {dropUnsupported && (
            <div className="app-window__notice">
              <Banner tone="warning" title={t("drop.unsupported.title")}>
                <p>{t("drop.unsupported.body")}</p>
                <div className="app-window__notice-actions">
                  <Button
                    onClick={() => {
                      setDropUnsupported(false);
                      setBatch({ text: "", note: null });
                      setDialog("batch");
                    }}
                  >
                    {t("drop.paste")}
                  </Button>
                  <Button
                    onClick={() => {
                      setDropUnsupported(false);
                      chooseListFile();
                    }}
                  >
                    {t("drop.import")}
                  </Button>
                  <Button onClick={() => setDropUnsupported(false)}>{t("drop.dismiss")}</Button>
                </div>
              </Banner>
            </div>
          )}
          {copied.length > 0 && !dialog && (
            <ClipboardPrompt
              links={copied}
              onDownload={() => {
                const links = copied;
                setCopied([]);
                openWithLinks(links);
              }}
              onDismiss={() => setCopied([])}
            />
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
            orderRef={tableOrder}
            onSelect={(id) => {
              setSelectedId(id);
              setProblem(null);
              setNotice(null);
            }}
          />
          <DownloadDetails
            selected={selected}
            problem={problem}
            onCancel={cancel.onClick}
            notice={notice}
            actions={{
              onRetry: retry.onClick,
              onRefreshLink: refreshLink.onClick,
              onOpenFolder: openFolder.onClick,
            }}
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
          initialUrl={addUrl}
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
      {dialog === "finish" && (
        <FinishActionDialog
          pending={pending}
          onClose={() => setDialog(null)}
          onArm={(action) => {
            setFinishFailed(false);
            setFinish(action);
            setDialog(null);
          }}
        />
      )}
      {finish !== null && countdown !== null && (
        <FinishCountdown
          action={finish}
          seconds={countdown}
          onCancel={() => {
            setFinish(null);
            setCountdown(null);
          }}
        />
      )}
      {dialog === "properties" && selected && (
        <PropertiesDialog
          download={selected}
          queues={list.queues}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "batch" && (
        <BatchAddDialog
          initialText={batch.text}
          note={batch.note}
          onClose={() => setDialog(null)}
          onDone={() => {
            setCategory("all");
            setProblem(null);
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
          copy={target.phase === "completed" ? REMOVE_FILE_COPY : REMOVE_COPY}
          option={target.phase === "completed" ? "remove.deleteFile" : undefined}
          run={(deleteFile) => {
            removedWithFile.current = deleteFile;
            return actions.remove(target.job_id, deleteFile);
          }}
          onDone={afterRemove}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "retry" && target && (
        <RetryDialog
          job={target}
          onDone={(job) => afterReopen(job, "announce.retried")}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "refresh" && target && (
        <RefreshLinkDialog
          job={target}
          onDone={(result) =>
            afterReopen(
              result.job,
              result.outcome === "progress_kept" ? "announce.linkKept" : "announce.linkRestarted",
            )
          }
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
      {dialog === "options" && (
        <OptionsDialog
          onClose={() => setDialog(null)}
          onManageQueues={() => {
            // Back to Options when Manage queues closes, so the explanation can be read again.
            queuesFromOptions.current = true;
            setDialog("queues");
          }}
        />
      )}
      {dialog === "queues" && (
        <ManageQueuesDialog
          onClose={() => {
            const back = queuesFromOptions.current;
            queuesFromOptions.current = false;
            setDialog(back ? "options" : null);
          }}
        />
      )}
      {dialog === "jobQueue" && selected && (
        <JobQueueDialog job={selected} onClose={() => setDialog(null)} />
      )}
      {dialog === "diagnostics" && <DiagnosticsDialog onClose={() => setDialog(null)} />}
      {dialog === "browsers" && <BrowserConnectionsDialog onClose={() => setDialog(null)} />}
      {dialog === "updates" && <UpdateDialog onClose={() => setDialog(null)} />}
      {dialog === "notices" && <NoticesDialog onClose={() => setDialog(null)} />}
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
          <p className="about__version">
            {format(t("app.version"), { version: __DOWNOVA_VERSION__ })}
          </p>
          <p>{t("about.body")}</p>
        </Dialog>
      )}
    </div>
  );
}
