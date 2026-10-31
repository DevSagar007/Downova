import { useRef, useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { newOperationId } from "./commands";
import { formatBytes } from "./present";

/**
 * Retry (CORE-03, ADR 0020) for one failed download. It always starts from zero: a failed
 * download has no resume point, so partial data is discarded, and the dialog says how much.
 *
 * The operation ID is fixed when the dialog opens. However often Retry is pressed — double
 * clicks, or again after an unconfirmed ("uncertain") answer — the same request is sent, so the
 * service starts at most one new attempt. "Keep as it is" sends nothing.
 */
export function RetryDialog({
  job,
  onDone,
  onClose,
}: {
  job: DownloadSnapshot;
  onDone: (job: DownloadSnapshot) => void;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const { retry } = useServiceActions();
  const [operationId] = useState(newOperationId);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const busy = useRef(false);
  const partial = BigInt(job.downloaded_bytes) > 0n;

  async function confirm() {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setFailure(null);
    try {
      onDone(
        await retry(job.job_id, operationId, {
          expectedAttemptId: job.attempt_id,
          discardPartial: partial,
        }),
      );
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  return (
    <Dialog
      title={t("retry.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("retry.keep")}</Button>
          <Button
            variant="primary"
            onClick={() => void confirm()}
            {...(pending ? { unavailableReason: t("retry.pending") } : {})}
          >
            {pending ? t("retry.pending") : t("retry.confirm")}
          </Button>
        </>
      }
    >
      <p>
        <bdi>{job.display_name}</bdi>
      </p>
      <p>{t("retry.body")}</p>
      {partial && <p>{format(t("retry.discard"), { size: formatBytes(job.downloaded_bytes) })}</p>}
      {job.checksum_expected && <p>{t("retry.checksum")}</p>}
      {failure && (
        <Banner tone="danger" title={t("retry.failed")}>
          <p>
            {failure.outcome === "uncertain"
              ? t("retry.uncertain")
              : failure.outcome === "unavailable"
                ? t("command.unavailable")
                : t(`error.kind.${failure.error.kind}`)}
          </p>
          {failure.outcome === "rejected" && failure.error.next_action !== "none" && (
            <p>{t(`error.action.${failure.error.next_action}`)}</p>
          )}
        </Banner>
      )}
    </Dialog>
  );
}
