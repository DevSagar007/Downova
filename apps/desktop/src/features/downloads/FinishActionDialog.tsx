import { useEffect, useId, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useShell } from "../../services/ServiceProvider";
import type { PowerAction, PowerSupport } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

const ACTIONS: PowerAction[] = ["sleep", "shutdown"];

/**
 * Downloads → When downloads finish… (F08). Chosen for this run only: after it fires or is
 * cancelled, nothing is remembered. An action the system does not offer is shown as unavailable.
 */
export function FinishActionDialog({
  pending,
  onArm,
  onClose,
}: {
  /** Downloads still to finish; with none, there is nothing to wait for. */
  pending: number;
  onArm: (action: PowerAction) => void;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const shell = useShell();
  const groupId = useId();
  const [support, setSupport] = useState<PowerSupport | null>(null);
  const [choice, setChoice] = useState<PowerAction | null>(null);
  useEffect(() => {
    let active = true;
    void shell.powerSupport().then((s) => {
      if (!active) return;
      setSupport(s);
      setChoice((c) => c ?? ACTIONS.find((a) => s[a]) ?? null);
    });
    return () => {
      active = false;
    };
  }, [shell]);

  const reason =
    pending === 0
      ? t("finish.nothingRunning")
      : !support || (!support.sleep && !support.shutdown)
        ? t("finish.unsupported")
        : choice === null
          ? t("finish.choose")
          : undefined;
  return (
    <Dialog
      title={t("finish.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("addUrl.cancel")}</Button>
          <Button
            variant="primary"
            onClick={() => choice && onArm(choice)}
            {...(reason ? { unavailableReason: reason } : {})}
          >
            {t("finish.arm")}
          </Button>
        </>
      }
    >
      <p>{t("finish.body")}</p>
      <fieldset className="finish-actions" aria-describedby={groupId}>
        <legend>{t("finish.legend")}</legend>
        {ACTIONS.map((action) => {
          const available = support?.[action] === true;
          return (
            <label key={action}>
              <input
                type="radio"
                name="finish-action"
                value={action}
                checked={choice === action}
                disabled={!available}
                onChange={() => setChoice(action)}
              />{" "}
              {t(`finish.action.${action}`)}
              {support && !available && ` (${t("finish.notOnThisSystem")})`}
            </label>
          );
        })}
      </fieldset>
      <p id={groupId} className="options-form__note">
        {pending === 0 ? t("finish.nothingRunning") : t("finish.note")}
      </p>
    </Dialog>
  );
}

/** The countdown before the computer powers down; Cancel keeps it running. */
export function FinishCountdown({
  action,
  seconds,
  onCancel,
}: {
  action: PowerAction;
  seconds: number;
  onCancel: () => void;
}) {
  const { t } = usePreferences();
  return (
    <Dialog
      title={t(`finish.countdown.${action}`)}
      closeLabel={t("finish.cancel")}
      onClose={onCancel}
      footer={
        <Button variant="primary" onClick={onCancel}>
          {t("finish.cancel")}
        </Button>
      }
    >
      <p role="timer" aria-live="off">
        {format(t("finish.countdown.body"), { seconds })}
      </p>
    </Dialog>
  );
}
