import { useEffect, useId, useRef, useState } from "react";
import type { DiagnosticsPreview } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import "./support.css";

type Saved =
  | { kind: "written"; fileName: string; size: string }
  | { kind: "cancelled" }
  | { kind: "failed"; failure: ServiceCallError }
  | null;

/**
 * Help → Diagnostics… (E05, ADR 0026). Shows the redacted report exactly as it would be saved,
 * then saves that same snapshot where the user chooses (the save dialog runs in the shell). Nothing
 * is sent anywhere; the crash-related section is off unless the user turns it on.
 */
export function DiagnosticsDialog({ onClose }: { onClose: () => void }) {
  const { t } = usePreferences();
  const service = useServiceState();
  const { previewDiagnostics, exportDiagnostics } = useServiceActions();
  const crashId = useId();
  const previewId = useId();
  const [includeCrash, setIncludeCrash] = useState(false);
  const [preview, setPreview] = useState<DiagnosticsPreview | null>(null);
  const [loadFailure, setLoadFailure] = useState<ServiceCallError | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<Saved>(null);
  // Only the latest request's answer is shown (a toggle or reconnect may overtake an older one).
  const seq = useRef(0);
  const busy = useRef(false);
  const connected = service.status === "connected";
  const instance = connected ? service.hello.service_instance_id : null;

  useEffect(() => {
    if (!instance) return;
    const mine = ++seq.current;
    previewDiagnostics(includeCrash)
      .then((p) => {
        if (mine !== seq.current) return;
        setPreview(p);
        setLoadFailure(null);
      })
      .catch((thrown: unknown) => {
        if (mine !== seq.current) return;
        setPreview(null);
        setLoadFailure(thrown as ServiceCallError);
      });
    return () => {
      seq.current += 1;
    };
  }, [instance, includeCrash, previewDiagnostics]);

  // Anything shown before a change of the switch or of the service is no longer what is saved.
  const current = preview && instance ? preview : null;

  async function save() {
    if (!current || busy.current) return;
    busy.current = true;
    setSaving(true);
    setSaved(null);
    const mine = seq.current;
    try {
      const out = await exportDiagnostics(current.snapshot_id, t("diagnostics.saveTitle"));
      if (mine !== seq.current) return;
      setSaved(
        out.outcome === "written"
          ? { kind: "written", fileName: out.file_name, size: out.size_bytes }
          : { kind: "cancelled" },
      );
    } catch (thrown) {
      if (mine === seq.current) setSaved({ kind: "failed", failure: thrown as ServiceCallError });
    } finally {
      busy.current = false;
      setSaving(false);
    }
  }

  const saveReason = !connected
    ? t("diagnostics.unavailable")
    : !current
      ? t("diagnostics.loading")
      : saving
        ? t("diagnostics.saving")
        : null;

  return (
    <Dialog
      title={t("diagnostics.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button
            variant="primary"
            onClick={() => void save()}
            {...(saveReason ? { unavailableReason: saveReason } : {})}
          >
            {saving ? t("diagnostics.saving") : t("diagnostics.save")}
          </Button>
          <Button onClick={onClose}>{t("options.done")}</Button>
        </>
      }
    >
      <div className="diagnostics">
        <p>{t("diagnostics.intro")}</p>
        <label className="diagnostics__check" htmlFor={crashId}>
          <input
            id={crashId}
            type="checkbox"
            checked={includeCrash}
            onChange={(e) => {
              setIncludeCrash(e.target.checked);
              setSaved(null);
            }}
          />
          {t("diagnostics.includeCrash")}
        </label>
        {!connected && <p role="status">{t("diagnostics.unavailable")}</p>}
        {loadFailure && (
          <Banner tone="danger" title={t("diagnostics.failed")}>
            <p>{t(`error.kind.${loadFailure.error.kind}`)}</p>
          </Banner>
        )}
        {current ? (
          <>
            <p id={`${previewId}-size`} className="diagnostics__size">
              {format(t("diagnostics.size"), { bytes: current.size_bytes })}
            </p>
            <textarea
              className="diagnostics__preview"
              readOnly
              rows={16}
              aria-label={t("diagnostics.preview")}
              aria-describedby={`${previewId}-size`}
              value={current.content}
            />
          </>
        ) : (
          connected && !loadFailure && <p role="status">{t("diagnostics.loading")}</p>
        )}
        {saved?.kind === "written" && (
          <Banner tone="success" title={t("diagnostics.savedTitle")}>
            <p>{format(t("diagnostics.saved"), { name: saved.fileName, bytes: saved.size })}</p>
          </Banner>
        )}
        {saved?.kind === "cancelled" && <p role="status">{t("diagnostics.cancelled")}</p>}
        {saved?.kind === "failed" && (
          <Banner tone="danger" title={t("diagnostics.saveFailed")}>
            <p>
              {saved.failure.outcome === "rejected"
                ? t(`error.kind.${saved.failure.error.kind}`)
                : t("command.uncertain")}
            </p>
          </Banner>
        )}
      </div>
    </Dialog>
  );
}
