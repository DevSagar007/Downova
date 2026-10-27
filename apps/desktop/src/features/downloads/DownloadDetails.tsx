import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import type { ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { errorText } from "./present";

export interface CommandProblem {
  command: "pause" | "resume";
  failure: ServiceCallError;
}

/**
 * Below the list: why the last Stop/Resume did not complete, otherwise the selected download's
 * error with what the user can do (CORE-11 copy, never the service's English detail).
 */
export function DownloadDetails({
  selected,
  problem,
}: {
  selected: DownloadSnapshot | undefined;
  problem: CommandProblem | null;
}) {
  const { t } = usePreferences();
  if (problem) {
    const { outcome, error } = problem.failure;
    const lines =
      outcome === "uncertain"
        ? [t("command.uncertain")]
        : outcome === "unavailable"
          ? [t("command.unavailable")]
          : [
              t(`error.kind.${error.kind}`),
              ...(error.next_action === "none" ? [] : [t(`error.action.${error.next_action}`)]),
            ];
    return (
      <div className="download-details">
        <Banner tone="danger" title={t(`command.${problem.command}Failed`)}>
          {lines.map((line) => (
            <p key={line}>{line}</p>
          ))}
        </Banner>
      </div>
    );
  }
  const text = selected ? errorText(selected, t) : null;
  if (!selected || !text) return null;
  const lines = [text.action, text.followUp].filter((l): l is string => l !== null);
  return (
    <div className="download-details">
      <Banner tone={selected.phase === "failed" ? "danger" : "warning"} title={text.summary}>
        {lines.length > 0 && lines.map((line) => <p key={line}>{line}</p>)}
      </Banner>
    </div>
  );
}
