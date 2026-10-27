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
  /** Why Delete is unavailable (removal semantics do not exist yet; it is never mapped to cancel). */
  deleteReason: string;
  /** Why commands that are not wired to the service yet are unavailable. */
  notWiredReason: string;
  onOptions: () => void;
}

/**
 * Main command toolbar. Add URL, Resume and Stop are wired to the service (B2.1); the others stay
 * visible but say why they are unavailable (design.md §6).
 */
export function DownloadsToolbar({
  add,
  resume,
  stop,
  deleteReason,
  notWiredReason,
  onOptions,
}: DownloadsToolbarProps) {
  const { t } = usePreferences();
  const needsService = notWiredReason;
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
        unavailableReason={needsService}
      />
      <span className="toolbar__separator" aria-hidden="true" />
      <ToolbarButton
        tone="delete"
        label={t("toolbar.delete")}
        icon={icon(Trash2)}
        unavailableReason={deleteReason}
      />
      <ToolbarButton
        tone="delete"
        label={t("toolbar.deleteCompleted")}
        icon={icon(ListChecks)}
        unavailableReason={needsService}
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
        unavailableReason={needsService}
      />
      <ToolbarButton
        tone="queue"
        label={t("toolbar.startQueue")}
        icon={icon(Play)}
        unavailableReason={needsService}
      />
      <ToolbarButton
        tone="queue"
        label={t("toolbar.stopQueue")}
        icon={icon(Square)}
        unavailableReason={needsService}
      />
    </div>
  );
}
