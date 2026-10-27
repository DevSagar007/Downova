import { useId, type ReactNode } from "react";
import "./ToolbarButton.css";

export type ToolbarTone = "add" | "resume" | "stop" | "delete" | "options" | "schedule" | "queue";

export interface ToolbarButtonProps {
  label: string;
  icon: ReactNode;
  tone: ToolbarTone;
  onClick?: (() => void) | undefined;
  /** When set, the command is unavailable: it stays focusable and explains why. */
  unavailableReason?: string | undefined;
}

/** Large icon-over-label command button used in the main toolbar (design.md §4). */
export function ToolbarButton({
  label,
  icon,
  tone,
  onClick,
  unavailableReason,
}: ToolbarButtonProps) {
  const reasonId = useId();
  const unavailable = unavailableReason !== undefined;
  return (
    <>
      <button
        type="button"
        className={`toolbar-button toolbar-button--${tone}`}
        aria-disabled={unavailable || undefined}
        aria-describedby={unavailable ? reasonId : undefined}
        title={unavailable ? `${label} — ${unavailableReason}` : label}
        onClick={unavailable ? undefined : onClick}
      >
        <span className="toolbar-button__icon" aria-hidden="true">
          {icon}
        </span>
        <span className="toolbar-button__label">{label}</span>
      </button>
      {unavailable && (
        <span id={reasonId} className="visually-hidden">
          {unavailableReason}
        </span>
      )}
    </>
  );
}
