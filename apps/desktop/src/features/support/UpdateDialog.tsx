import { useCallback, useEffect, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useServiceActions, useShell } from "../../services/ServiceProvider";
import { toCallError, type UpdateStatus, type UpdateView } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import "./support.css";

/** Help → Check for updates… automatic check (opt-in, off by default). */
export const AUTO_CHECK_KEY = "downova.updates.autoCheck";

export function readAutoCheck(): boolean {
  try {
    return localStorage.getItem(AUTO_CHECK_KEY) === "1";
  } catch {
    return false;
  }
}

function writeAutoCheck(on: boolean) {
  try {
    if (on) localStorage.setItem(AUTO_CHECK_KEY, "1");
    else localStorage.removeItem(AUTO_CHECK_KEY);
  } catch {
    // Storage may be unavailable; the choice then lasts for this window only.
  }
}

const opOf = (v: UpdateView): string | null => ("operation_id" in v ? v.operation_id : null);

/**
 * Help → Check for updates… (ADV09, ADR 0030). Shows the shell's update state and offers only
 * the next step: check → download and verify → prepare (the service saves progress and backs up)
 * → install after an explicit confirmation. "Not configured" is never shown as "up to date", and
 * "verified" only after the signature and signed version passed. One step at a time: a click
 * while a step runs does nothing, and an answer for an older operation is never shown.
 */
export function UpdateDialog({ onClose }: { onClose: () => void }) {
  const { t } = usePreferences();
  const shell = useShell();
  const { reconnect } = useServiceActions();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [view, setView] = useState<UpdateView | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState<"install" | "restore" | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [autoCheck, setAutoCheck] = useState(readAutoCheck);
  const inFlight = useRef(false);
  // The operation the window is following; events or answers for another one are ignored.
  const current = useRef<string | null>(null);
  const open = useRef(true);

  const show = useCallback((next: UpdateView) => {
    const op = opOf(next);
    // A new check starts a new operation; anything else must belong to the current one.
    if (op && current.current && op !== current.current && next.state !== "checking") return;
    if (op) current.current = op;
    setView(next);
  }, []);

  useEffect(() => {
    open.current = true;
    void shell.updateStatus().then(
      (s) => {
        if (!open.current) return;
        setStatus(s);
        current.current = opOf(s.view);
        setView(s.view);
      },
      () => open.current && setView({ state: "not_configured", reason: "unpackaged" }),
    );
    const off = shell.onUpdate((v) => open.current && show(v));
    return () => {
      open.current = false;
      off();
    };
  }, [shell, show]);

  const run = (step: () => Promise<UpdateView>, newOperation = false) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setProblem(null);
    setConfirming(null);
    if (newOperation) current.current = null;
    step()
      .then(
        (v) => open.current && show(v),
        (e: unknown) => open.current && setProblem(t(`error.kind.${toCallError(e).error.kind}`)),
      )
      .finally(() => {
        inFlight.current = false;
        if (open.current) setBusy(false);
      });
  };

  const op = view ? opOf(view) : null;
  const unavailable = busy ? { unavailableReason: t("updates.busy") } : {};
  const check = () => run(() => shell.updateCheck(), true);
  const cancel = () => op && run(() => shell.updateCancel(op));

  const body = (() => {
    if (!view) return <p role="status">{t("updates.loading")}</p>;
    switch (view.state) {
      case "not_configured":
        return (
          <Banner tone="info" title={t("updates.notConfigured.title")}>
            <p>{t(`updates.notConfigured.${view.reason}` as MessageKey)}</p>
          </Banner>
        );
      case "idle":
        return <p>{t("updates.idle")}</p>;
      case "checking":
        return <p role="status">{t("updates.checking")}</p>;
      case "no_update":
        return (
          <p role="status">
            {format(t("updates.noUpdate"), { version: status?.current_version ?? "" })}
          </p>
        );
      case "available":
        return (
          <>
            <p role="status">{format(t("updates.available"), { version: view.version })}</p>
            {view.notes && <p className="updates__notes">{view.notes}</p>}
            <p>{t("updates.downloadNote")}</p>
          </>
        );
      case "downloading": {
        const pct =
          view.total && view.total > 0
            ? Math.min(100, Math.floor((view.downloaded / view.total) * 100))
            : null;
        return (
          <>
            <p role="status">{format(t("updates.downloading"), { version: view.version })}</p>
            <progress
              className="updates__progress"
              aria-label={t("updates.progressLabel")}
              {...(pct === null ? {} : { value: pct, max: 100 })}
            />
          </>
        );
      }
      case "verifying":
        return <p role="status">{t("updates.verifying")}</p>;
      case "verified":
        return (
          <>
            <Banner tone="success" title={t("updates.verified.title")}>
              <p>{format(t("updates.verified"), { version: view.version })}</p>
            </Banner>
            <p>{t("updates.prepareNote")}</p>
          </>
        );
      case "preparing":
        return <p role="status">{t("updates.preparing")}</p>;
      case "ready_to_install":
        return (
          <>
            <Banner tone="success" title={t("updates.ready.title")}>
              <p>
                {format(t("updates.ready"), {
                  version: view.version,
                  held: view.prepared.held_jobs,
                })}
              </p>
            </Banner>
            <p>{t("updates.installNote")}</p>
            {confirming === "install" && (
              <p role="alert">{format(t("updates.confirmInstall"), { version: view.version })}</p>
            )}
          </>
        );
      case "installing":
        return <p role="status">{t("updates.installing")}</p>;
      case "recovery_required":
        return (
          <Banner tone="warning" title={t("updates.recovery.title")}>
            <p>{t(`updates.recovery.${view.kind}` as MessageKey)}</p>
            <p>
              {view.backup_operation
                ? t("updates.recovery.backup")
                : t("updates.recovery.noBackup")}
            </p>
            {confirming === "restore" && <p role="alert">{t("updates.confirmRestore")}</p>}
          </Banner>
        );
      case "failed":
        return (
          <Banner tone="danger" title={t(`updates.failed.${view.stage}` as MessageKey)}>
            <p>{t(`error.kind.${view.error.kind}`)}</p>
            <p>{t("updates.failedNote")}</p>
          </Banner>
        );
    }
  })();

  const actions = (() => {
    if (!view) return null;
    switch (view.state) {
      case "idle":
      case "no_update":
        return (
          <Button variant="primary" onClick={check} {...unavailable}>
            {t("updates.check")}
          </Button>
        );
      case "available":
        return (
          <>
            <Button onClick={cancel} {...unavailable}>
              {t("updates.notNow")}
            </Button>
            <Button
              variant="primary"
              onClick={() => run(() => shell.updateDownload(view.operation_id))}
              {...unavailable}
            >
              {t("updates.download")}
            </Button>
          </>
        );
      case "verified":
        return (
          <>
            <Button onClick={cancel} {...unavailable}>
              {t("updates.notNow")}
            </Button>
            <Button
              variant="primary"
              onClick={() => run(() => shell.updatePrepare(view.operation_id))}
              {...unavailable}
            >
              {t("updates.prepare")}
            </Button>
          </>
        );
      case "ready_to_install":
        return (
          <>
            <Button onClick={cancel} {...unavailable}>
              {t("updates.notNow")}
            </Button>
            {confirming === "install" ? (
              <Button
                variant="primary"
                onClick={() => run(() => shell.updateInstall(view.operation_id))}
                {...unavailable}
              >
                {t("updates.installNow")}
              </Button>
            ) : (
              <Button variant="primary" onClick={() => setConfirming("install")} {...unavailable}>
                {t("updates.install")}
              </Button>
            )}
          </>
        );
      case "recovery_required": {
        const backup = view.backup_operation;
        return (
          <>
            <Button onClick={() => reconnect()} {...unavailable}>
              {t("updates.recovery.retry")}
            </Button>
            {backup &&
              (confirming === "restore" ? (
                <Button
                  variant="primary"
                  onClick={() => run(() => shell.updateRestore(backup))}
                  {...unavailable}
                >
                  {t("updates.recovery.restoreNow")}
                </Button>
              ) : (
                <Button variant="primary" onClick={() => setConfirming("restore")} {...unavailable}>
                  {t("updates.recovery.restore")}
                </Button>
              ))}
          </>
        );
      }
      case "failed":
        return (
          <>
            {view.stage === "shutdown" || view.stage === "install" ? (
              <Button onClick={() => reconnect()} {...unavailable}>
                {t("updates.reconnect")}
              </Button>
            ) : null}
            {view.can_retry_prepare && view.operation_id ? (
              <Button
                onClick={() => run(() => shell.updatePrepare(view.operation_id as string))}
                {...unavailable}
              >
                {t("updates.prepareAgain")}
              </Button>
            ) : null}
            {view.operation_id ? (
              <Button onClick={cancel} {...unavailable}>
                {t("updates.notNow")}
              </Button>
            ) : null}
            <Button variant="primary" onClick={check} {...unavailable}>
              {t("updates.check")}
            </Button>
          </>
        );
      default:
        return null;
    }
  })();

  return (
    <Dialog
      title={t("updates.title")}
      onClose={onClose}
      closeLabel={t("options.close")}
      footer={
        <>
          {actions}
          <Button onClick={onClose}>{t("options.close")}</Button>
        </>
      }
    >
      <div className="updates">
        {status && (
          <p>
            {format(t("updates.current"), { version: status.current_version })}
            {status.fixture ? ` — ${t("updates.fixture")}` : ""}
          </p>
        )}
        {body}
        {problem && <p role="alert">{problem}</p>}
        {view?.state !== "not_configured" && (
          <label className="updates__auto">
            <input
              type="checkbox"
              checked={autoCheck}
              onChange={(e) => {
                setAutoCheck(e.target.checked);
                writeAutoCheck(e.target.checked);
              }}
            />{" "}
            {t("updates.autoCheck")}
          </label>
        )}
      </div>
    </Dialog>
  );
}
