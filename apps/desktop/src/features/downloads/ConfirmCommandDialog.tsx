import { useRef, useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import type { MessageKey } from "../../i18n/messages";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

export interface ConfirmCopy {
  title: MessageKey;
  body: MessageKey;
  confirm: MessageKey;
  keep: MessageKey;
  pending: MessageKey;
  failed: MessageKey;
}

export const CANCEL_COPY: ConfirmCopy = {
  title: "cancel.title",
  body: "cancel.body",
  confirm: "cancel.confirm",
  keep: "cancel.keep",
  pending: "cancel.pending",
  failed: "cancel.failed",
};

export const REMOVE_COPY: ConfirmCopy = {
  title: "remove.title",
  body: "remove.body",
  confirm: "remove.confirm",
  keep: "remove.keep",
  pending: "remove.pending",
  failed: "remove.failed",
};

/**
 * Confirmation for one row command (Cancel download, Remove from list). "Keep" sends nothing.
 * Confirm sends one request however often it is pressed; the dialog closes only on the service's
 * acknowledgement and otherwise explains the failure with the error catalogue.
 */
export function ConfirmCommandDialog({
  name,
  copy,
  run,
  onDone,
  onClose,
}: {
  name: string;
  copy: ConfirmCopy;
  run: () => Promise<DownloadSnapshot>;
  onDone: (job: DownloadSnapshot) => void;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const busy = useRef(false);

  async function confirm() {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setFailure(null);
    try {
      onDone(await run());
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  return (
    <Dialog
      title={t(copy.title)}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t(copy.keep)}</Button>
          <Button
            variant="primary"
            onClick={() => void confirm()}
            {...(pending ? { unavailableReason: t(copy.pending) } : {})}
          >
            {pending ? t(copy.pending) : t(copy.confirm)}
          </Button>
        </>
      }
    >
      <p>
        <bdi>{name}</bdi>
      </p>
      <p>{t(copy.body)}</p>
      {failure && (
        <Banner tone="danger" title={t(copy.failed)}>
          <p>
            {failure.outcome === "uncertain"
              ? t("command.uncertainState")
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
