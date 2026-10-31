import { useEffect, useId, useState } from "react";
import type { HistoryPreview } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";

/** Offered periods; `null` keeps history indefinitely (the default). */
const CHOICES: (number | null)[] = [null, 7, 30, 90, 365];

type Pending =
  | { kind: "apply"; days: number | null; preview: HistoryPreview }
  | {
      kind: "clean";
      preview: HistoryPreview;
    };
type Outcome =
  | { kind: "saved" }
  | { kind: "cleaned"; removed: number; kept: number }
  | { kind: "failed"; failure: ServiceCallError };

/**
 * Options → History (CORE-10, ADR 0018): how long finished downloads stay in the list. Retention
 * is applied by the service; removing a list entry never deletes a downloaded or partial file.
 * Anything that would remove entries now first shows the service's exact count and asks.
 */
export function HistorySettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getHistory, setHistory, previewHistory, cleanHistory } = useServiceActions();
  const headingId = useId();
  const selectId = useId();
  const noteId = useId();
  const connected = service.status === "connected";
  const [saved, setSaved] = useState<number | null | undefined>(undefined);
  const [choice, setChoice] = useState<number | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  useEffect(() => {
    if (!connected) return;
    let active = true;
    getHistory()
      .then((h) => {
        if (!active) return;
        setSaved(h.retention_days);
        setChoice(h.retention_days);
      })
      .catch(() => {
        if (active) setSaved(undefined);
      });
    return () => {
      active = false;
    };
  }, [connected, getHistory]);

  const label = (days: number | null) =>
    days === null
      ? t("history.forever")
      : days === 365
        ? t("history.year")
        : format(t("history.days"), { count: days });

  /** Runs one step; `null` means "nothing to report yet" (a confirmation is showing). */
  async function run(work: () => Promise<Outcome | null>) {
    if (busy) return;
    setBusy(true);
    setOutcome(null);
    try {
      setOutcome(await work());
    } catch (thrown) {
      setOutcome({ kind: "failed", failure: toCallError(thrown, true) });
    } finally {
      setBusy(false);
    }
  }

  /** Apply the chosen period: ask first when it would remove entries now. */
  function apply() {
    void run(async () => {
      const preview = await previewHistory(choice);
      if (preview.expiring > 0) {
        setPending({ kind: "apply", days: choice, preview });
        return null;
      }
      const h = await setHistory(choice);
      setSaved(h.retention_days);
      return { kind: "saved" };
    });
  }

  function cleanNow() {
    void run(async () => {
      setPending({ kind: "clean", preview: await previewHistory(saved ?? null) });
      return null;
    });
  }

  function confirm() {
    const p = pending;
    if (!p) return;
    setPending(null);
    void run(async () => {
      if (p.kind === "apply") {
        const h = await setHistory(p.days);
        setSaved(h.retention_days);
      }
      const r = await cleanHistory();
      return { kind: "cleaned", removed: r.removed, kept: r.kept };
    });
  }

  const busyReason = busy ? { unavailableReason: t("unavailable.busy") } : {};
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("history.heading")}
      </h3>
      {!connected || saved === undefined ? (
        <p className="options-form__note">{t("history.unavailable")}</p>
      ) : (
        <>
          <div className="options-field">
            <label htmlFor={selectId}>{t("history.keep")}</label>
            <select
              id={selectId}
              value={choice === null ? "" : String(choice)}
              aria-describedby={noteId}
              onChange={(e) => {
                setPending(null);
                setOutcome(null);
                setChoice(e.target.value === "" ? null : Number(e.target.value));
              }}
            >
              {CHOICES.map((days) => (
                <option key={String(days)} value={days === null ? "" : String(days)}>
                  {label(days)}
                </option>
              ))}
            </select>
          </div>
          <p id={noteId} className="options-form__note">
            {t("history.note")}
          </p>
          <div className="options-actions">
            <Button
              onClick={apply}
              {...(choice === saved ? { unavailableReason: t("history.unchanged") } : busyReason)}
            >
              {t("history.apply")}
            </Button>
            <Button
              onClick={cleanNow}
              {...(saved === null ? { unavailableReason: t("history.cleanForever") } : busyReason)}
            >
              {t("history.clean")}
            </Button>
          </div>
          {pending && (
            <div role="alert">
              <Banner tone="warning" title={t("history.confirmTitle")}>
                <p>
                  {pending.preview.expiring === 0
                    ? t("history.confirmNone")
                    : format(t("history.confirmBody"), {
                        count: pending.preview.expiring,
                        total: pending.preview.finished,
                      })}
                </p>
                <p>{t("history.filesStay")}</p>
                <div className="options-actions">
                  <Button
                    variant="primary"
                    onClick={confirm}
                    {...(pending.preview.expiring === 0 && pending.kind === "clean"
                      ? { unavailableReason: t("history.confirmNone") }
                      : {})}
                  >
                    {pending.kind === "apply"
                      ? t("history.confirmApply")
                      : t("history.confirmClean")}
                  </Button>
                  <Button onClick={() => setPending(null)}>{t("history.keepThem")}</Button>
                </div>
              </Banner>
            </div>
          )}
          {outcome?.kind === "saved" && (
            <p role="status" className="options-form__note">
              {t("history.saved")}
            </p>
          )}
          {outcome?.kind === "cleaned" && (
            <p role="status" className="options-form__note">
              {format(t("history.cleaned"), { count: outcome.removed })}
              {outcome.kept > 0 ? ` ${format(t("history.kept"), { count: outcome.kept })}` : ""}
            </p>
          )}
          {outcome?.kind === "failed" && (
            <div role="alert" className="options-form__note">
              <p>{t("history.failed")}</p>
              {outcome.failure.outcome === "rejected" ? (
                <p>{t(`error.kind.${outcome.failure.error.kind}`)}</p>
              ) : (
                <p>{t("command.uncertain")}</p>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
