import { useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { useServiceActions, useShell } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

type Result =
  | { kind: "stopped"; paused: number }
  | { kind: "stillRunning"; paused: number }
  | { kind: "failed"; failure: ServiceCallError };

/**
 * The one confirmation for "Exit and stop downloads" (UI-04, ADR 0012), opened from the Tasks
 * menu or the tray. Cancel changes nothing. Confirm sends one request; the dialog reports whether
 * the service was verified stopped, merely acknowledged, or refused.
 */
export function ExitDialog({ onClose }: { onClose: () => void }) {
  const { t } = usePreferences();
  const shell = useShell();
  const { beginExit, cancelExit } = useServiceActions();
  const [pending, setPending] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const busy = useRef(false);

  async function confirm() {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setResult(null);
    // No live updates or automatic reconnection while (and after) the service stops: a lost
    // stream must never look like a crash that needs reconnecting (B2.10).
    beginExit();
    try {
      const report = await shell.exitAndStop();
      setResult({
        kind: report.stopped ? "stopped" : "stillRunning",
        paused: report.paused_jobs,
      });
      if (!report.stopped) cancelExit();
    } catch (thrown) {
      setResult({ kind: "failed", failure: toCallError(thrown, true) });
      // Nothing was stopped for certain: show the service's state again (attach only).
      cancelExit();
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  const done = result?.kind === "stopped";
  return (
    <Dialog
      title={t("exit.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        done ? null : (
          <>
            <Button onClick={onClose}>{t("exit.cancel")}</Button>
            <Button
              variant="primary"
              onClick={() => void confirm()}
              {...(pending ? { unavailableReason: t("exit.stopping") } : {})}
            >
              {pending ? t("exit.stopping") : t("exit.confirm")}
            </Button>
          </>
        )
      }
    >
      <p>{t("exit.body")}</p>
      {result && <ResultBanner result={result} />}
    </Dialog>
  );
}

function ResultBanner({ result }: { result: Result }) {
  const { t } = usePreferences();
  if (result.kind === "stopped") {
    return <Banner tone="success" title={t("exit.stopped")} />;
  }
  if (result.kind === "stillRunning") {
    return <Banner tone="warning" title={t("exit.stillRunning")} />;
  }
  const { outcome, error } = result.failure;
  const body =
    outcome === "uncertain"
      ? t("exit.uncertain")
      : outcome === "unavailable"
        ? t("exit.unavailable")
        : t(`error.kind.${error.kind}`);
  return (
    <Banner tone="danger" title={t("exit.failed")}>
      <p>{body}</p>
    </Banner>
  );
}
