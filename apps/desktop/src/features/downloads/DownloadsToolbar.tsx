import {
  CalendarClock,
  CirclePause,
  CirclePlay,
  ListChecks,
  OctagonX,
  Play,
  Plus,
  Settings2,
  Square,
  Trash2,
} from "lucide-react";
import type { KeyboardEvent } from "react";
import { usePreferences } from "../../app/preferences";
import { ToolbarButton } from "../../ui/ToolbarButton";

/** A wired command: either it runs, or it says why it cannot. */
export interface ToolbarCommand {
  onClick?: () => void;
  unavailableReason?: string | undefined;
}

export interface DownloadsToolbarProps {
  add: ToolbarCommand;
  resume: ToolbarCommand;
  stop: ToolbarCommand;
  /** Start or stop the current queue (B2.11): the selected queue, else the selected job's. */
  startQueue: ToolbarCommand;
  stopQueue: ToolbarCommand;
  /** Delete: the Remove confirmation for the selection, which can also delete the file. */
  deleteCommand: ToolbarCommand;
  /** Pauses every download that can be paused. */
  stopAll: ToolbarCommand;
  /** Removes finished downloads from the list after a confirmation (the files stay). */
  deleteCompleted: ToolbarCommand;
  /** Opens the queues, where schedules are set. */
  scheduler: ToolbarCommand;
  onOptions: () => void;
}

/**
 * Main command toolbar. Every command is wired; one that cannot run now says why (design.md §6).
 */
export function DownloadsToolbar({
  add,
  resume,
  stop,
  startQueue,
  stopQueue,
  deleteCommand,
  stopAll,
  deleteCompleted,
  scheduler,
  onOptions,
}: DownloadsToolbarProps) {
  const { t } = usePreferences();
  const icon = (Icon: typeof Plus) => <Icon className="icon" aria-hidden="true" />;

  // Arrow keys move between toolbar commands (ARIA toolbar pattern); Tab still works too.
  function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    const buttons = Array.from(event.currentTarget.querySelectorAll("button"));
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    event.preventDefault();
    const delta = event.key === "ArrowRight" ? 1 : -1;
    buttons[(index + delta + buttons.length) % buttons.length]?.focus();
  }

  return (
    <div className="toolbar" role="toolbar" aria-label={t("toolbar.label")} onKeyDown={moveFocus}>
      <ToolbarButton
        tone="add"
        label={t("toolbar.addUrl")}
        icon={icon(Plus)}
        onClick={add.onClick}
        unavailableReason={add.unavailableReason}
      />
      <ToolbarButton
        tone="resume"
        label={t("toolbar.resume")}
        icon={icon(CirclePlay)}
        onClick={resume.onClick}
        unavailableReason={resume.unavailableReason}
      />
      <ToolbarButton
        tone="stop"
        label={t("toolbar.stop")}
        icon={icon(CirclePause)}
        onClick={stop.onClick}
        unavailableReason={stop.unavailableReason}
      />
      <ToolbarButton
        tone="stop"
        label={t("toolbar.stopAll")}
        icon={icon(OctagonX)}
        onClick={stopAll.onClick}
        unavailableReason={stopAll.unavailableReason}
      />
      <span className="toolbar__separator" aria-hidden="true" />
      <ToolbarButton
        tone="delete"
        label={t("toolbar.delete")}
        icon={icon(Trash2)}
        onClick={deleteCommand.onClick}
        unavailableReason={deleteCommand.unavailableReason}
      />
      <ToolbarButton
        tone="delete"
        label={t("toolbar.deleteCompleted")}
        icon={icon(ListChecks)}
        onClick={deleteCompleted.onClick}
        unavailableReason={deleteCompleted.unavailableReason}
      />
      <span className="toolbar__separator" aria-hidden="true" />
      <ToolbarButton
        tone="options"
        label={t("toolbar.options")}
        icon={icon(Settings2)}
        onClick={onOptions}
      />
      <ToolbarButton
        tone="schedule"
        label={t("toolbar.scheduler")}
        icon={icon(CalendarClock)}
        onClick={scheduler.onClick}
        unavailableReason={scheduler.unavailableReason}
      />
      <ToolbarButton
        tone="queue"
        label={t("toolbar.startQueue")}
        icon={icon(Play)}
        onClick={startQueue.onClick}
        unavailableReason={startQueue.unavailableReason}
      />
      <ToolbarButton
        tone="queue"
        label={t("toolbar.stopQueue")}
        icon={icon(Square)}
        onClick={stopQueue.onClick}
        unavailableReason={stopQueue.unavailableReason}
      />
    </div>
  );
}
