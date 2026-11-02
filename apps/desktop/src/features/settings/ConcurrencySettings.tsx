import { useEffect, useId, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";

/** Global admission is service-owned; this control never silently changes a queue. */
export function ConcurrencySettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getConcurrency, setConcurrency } = useServiceActions();
  const instance = service.status === "connected" ? service.hello.service_instance_id : null;
  const headingId = useId();
  const fieldId = useId();
  const noteId = useId();
  const errorId = useId();
  const [loaded, setLoaded] = useState<{
    connection: ReturnType<typeof useServiceState>;
    revision: number;
    limit: number;
  } | null>(null);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const epoch = useRef(0);
  const busy = useRef(false);

  useEffect(() => {
    const current = ++epoch.current;
    if (instance) {
      getConcurrency()
        .then((settings) => {
          if (current !== epoch.current) return;
          setLoaded({
            connection: service,
            revision: reload,
            limit: settings.max_active_downloads,
          });
          setValue(String(settings.max_active_downloads));
          setFailure(null);
          setLoadFailed(false);
          setSaved(false);
          setSaving(false);
          busy.current = false;
        })
        .catch((thrown: unknown) => {
          if (current !== epoch.current) return;
          setLoaded(null);
          setLoadFailed(true);
          setFailure(toCallError(thrown));
          setSaving(false);
          busy.current = false;
        });
    }
    return () => {
      epoch.current = current + 1;
    };
  }, [instance, service, getConcurrency, reload]);

  const ready = instance !== null && loaded?.connection === service && loaded.revision === reload;
  const valid = /^(?:[1-9]|1[0-6])$/.test(value);
  const invalid = ready && !valid;
  const saveReason = !ready
    ? t("concurrency.unavailable")
    : saving
      ? t("concurrency.saving")
      : invalid
        ? t("concurrency.invalid")
        : null;

  async function save() {
    if (!ready || !valid || busy.current || !instance) return;
    const current = epoch.current;
    busy.current = true;
    setSaving(true);
    setSaved(false);
    setFailure(null);
    try {
      const settings = await setConcurrency(Number(value));
      if (current !== epoch.current) return;
      setLoaded({ connection: service, revision: reload, limit: settings.max_active_downloads });
      setValue(String(settings.max_active_downloads));
      setSaved(true);
    } catch (thrown) {
      if (current === epoch.current) setFailure(toCallError(thrown, true));
    } finally {
      if (current === epoch.current) {
        busy.current = false;
        setSaving(false);
      }
    }
  }

  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("concurrency.heading")}
      </h3>
      <form
        className="options-form"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="options-field">
          <label htmlFor={fieldId}>{t("concurrency.limit")}</label>
          <input
            id={fieldId}
            type="number"
            inputMode="numeric"
            min={1}
            max={16}
            step={1}
            value={ready ? value : ""}
            disabled={!ready || saving}
            aria-invalid={invalid || undefined}
            aria-describedby={`${noteId}${invalid ? ` ${errorId}` : ""}`}
            onChange={(e) => {
              setValue(e.target.value);
              setSaved(false);
            }}
          />
        </div>
        <p id={noteId} className="options-form__note">
          {t("concurrency.note")}
        </p>
        {service.status === "connected" && service.hello.backend.id === "aria2" && (
          <p className="options-form__note">{t("concurrency.aria2")}</p>
        )}
        {invalid && (
          <p id={errorId} role="alert" className="options-form__note">
            {t("concurrency.invalid")}
          </p>
        )}
        {!ready && !loadFailed && (
          <p className="options-form__note">
            {t(instance ? "concurrency.loading" : "concurrency.unavailable")}
          </p>
        )}
        {failure && (
          <p role="alert" className="options-form__note">
            {failure.outcome === "uncertain" ? t("concurrency.uncertain") : t("concurrency.failed")}
          </p>
        )}
        {instance && (loadFailed || failure?.outcome === "uncertain") && (
          <Button
            onClick={() => setReload((n) => n + 1)}
            {...(saving ? { unavailableReason: t("concurrency.saving") } : {})}
          >
            {t("concurrency.reload")}
          </Button>
        )}
        <Button type="submit" {...(saveReason ? { unavailableReason: saveReason } : {})}>
          {t(saving ? "concurrency.saving" : "concurrency.save")}
        </Button>
        {saved && ready && (
          <p role="status" aria-label={t("concurrency.status")} className="options-form__note">
            {t("concurrency.saved")} {loaded.limit}
          </p>
        )}
      </form>
    </section>
  );
}
