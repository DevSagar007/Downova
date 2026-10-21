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

export interface DownloadsToolbarProps {
  connected: boolean;
  onOptions: () => void;
}

/**
 * Main command toolbar. Commands that need the download service stay visible but report why they
 * are unavailable until the service is connected (design.md §6).
 */
export function DownloadsToolbar({ connected, onOptions }: DownloadsToolbarProps) {
  const { t } = usePreferences();
  const needsService = connected ? undefined : t("unavailable.service");
  const needsSelection = connected ? t("unavailable.selection") : t("unavailable.service");
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
        unavailableReason={needsService}
      />
      <ToolbarButton
        tone="resume"
        label={t("toolbar.resume")}
        icon={icon(CirclePlay)}
        unavailableReason={needsSelection}
      />
      <ToolbarButton
        tone="stop"
        label={t("toolbar.stop")}
        icon={icon(CirclePause)}
        unavailableReason={needsSelection}
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
        unavailableReason={needsSelection}
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
