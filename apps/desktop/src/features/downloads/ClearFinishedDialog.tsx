import { useRef, useState } from "react";
import type { DownloadSnapshot, JobId } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { clearFinished, type ClearFailure, type ClearOutcome, type ClearTarget } from "./clear";

function failureText(failure: ClearFailure, t: (key: MessageKey) => string): string {
  const { outcome, error } = failure.error;
  if (outcome === "uncertain") return t("command.uncertainState");
  if (outcome === "unavailable") return t("command.unavailable");
  return [
    t(`error.kind.${error.kind}`),
    ...(error.next_action === "none" ? [] : [t(`error.action.${error.next_action}`)]),
  ].join(" ");
}

/**
 * One confirmation for every finished download that existed when it opened (CORE-10). The scope is
 * frozen by the caller. Requests run with bounded concurrency; a partial failure lists the rows
 * that were kept and their actionable reasons. Confirm runs once.
 */
export function ClearFinishedDialog({
  targets,
  remove,
  onCompleted,
  onClose,
}: {
  targets: readonly ClearTarget[];
  remove: (jobId: JobId) => Promise<DownloadSnapshot>;
  onCompleted: (outcome: ClearOutcome) => void;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const [pending, setPending] = useState(false);
  const [outcome, setOutcome] = useState<ClearOutcome | null>(null);
  const busy = useRef(false);

  async function confirm() {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    try {
      const result = await clearFinished(targets, remove);
      setOutcome(result);
      onCompleted(result);
    } finally {
      setPending(false);
    }
  }

  return (
    <Dialog
      title={t("clear.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        outcome ? (
          <Button onClick={onClose}>{t("options.close")}</Button>
        ) : (
          <>
            <Button onClick={onClose}>{t("clear.keep")}</Button>
            <Button
              variant="primary"
              onClick={() => void confirm()}
              {...(pending ? { unavailableReason: t("clear.pending") } : {})}
            >
              {pending ? t("clear.pending") : t("clear.confirm")}
            </Button>
          </>
        )
      }
    >
      <p>{format(t("clear.body"), { count: targets.length })}</p>
      {outcome && (
        <Banner tone="warning" title={t("clear.failedTitle")}>
          <p role="status">
            {format(t("clear.result"), {
              removed: outcome.succeeded.length,
              failed: outcome.failed.length,
              total: outcome.succeeded.length + outcome.failed.length,
            })}
          </p>
          <ul>
            {outcome.failed.map((failure) => (
              <li key={failure.jobId}>
                <bdi>{failure.name}</bdi>: {failureText(failure, t)}
              </li>
            ))}
          </ul>
        </Banner>
      )}
    </Dialog>
  );
}
