import { useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
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
  onCancel,
}: {
  selected: DownloadSnapshot | undefined;
  problem: CommandProblem | null;
  /** Opens the Cancel confirmation for the selected job. */
  onCancel?: (() => void) | undefined;
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
  if (selected?.blockers.includes("name_conflict")) {
    return <NameConflict job={selected} onCancel={onCancel} />;
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

/**
 * Ask-on-collision (ADR 0015): the verified file waits under its staging name until the user
 * accepts the free name the service chose, or cancels. Nothing is ever overwritten.
 */
function NameConflict({
  job,
  onCancel,
}: {
  job: DownloadSnapshot;
  onCancel?: (() => void) | undefined;
}) {
  const { t } = usePreferences();
  const { confirmName } = useServiceActions();
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const name = job.display_name;
  async function confirm() {
    if (pending) return;
    setPending(true);
    setFailure(null);
    try {
      await confirmName(job.job_id);
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="download-details">
      <Banner tone="warning" title={t("nameConflict.title")}>
        <p>{format(t("nameConflict.body"), { name })}</p>
        {failure && (
          <p role="alert">
            {t("nameConflict.failed")}{" "}
            {failure.outcome === "rejected"
              ? t(`error.kind.${failure.error.kind}`)
              : t("command.uncertain")}
          </p>
        )}
        <div className="download-details__actions">
          <Button
            variant="primary"
            onClick={() => void confirm()}
            {...(pending ? { unavailableReason: t("nameConflict.pending") } : {})}
          >
            {pending ? t("nameConflict.pending") : format(t("nameConflict.confirm"), { name })}
          </Button>
          {onCancel && <Button onClick={onCancel}>{t("cancel.confirm")}</Button>}
        </div>
      </Banner>
    </div>
  );
}
