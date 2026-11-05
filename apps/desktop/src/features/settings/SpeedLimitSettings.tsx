import { useEffect, useId, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import {
  RATE_UNITS,
  engineSupports,
  formatRate,
  splitRate,
  toBytesPerSecond,
  type RateUnit,
} from "./speed";

/**
 * The overall bandwidth limit (CORE-06, ADR 0024): service-owned, saved before it applies, shown
 * from the service's answer (never the draft). Running downloads follow a change at once.
 */
export function SpeedLimitSettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getSpeedLimit, setSpeedLimit } = useServiceActions();
  const instance = service.status === "connected" ? service.hello.service_instance_id : null;
  const supported =
    service.status === "connected" && engineSupports(service.hello.backend, "speed_limit_global");
  const headingId = useId();
  const limitId = useId();
  const valueId = useId();
  const unitId = useId();
  const noteId = useId();
  const errorId = useId();
  const [loaded, setLoaded] = useState<{
    connection: ReturnType<typeof useServiceState>;
    revision: number;
    bps: string | null;
  } | null>(null);
  const [limited, setLimited] = useState(false);
  const [value, setValue] = useState("");
  const [unit, setUnit] = useState<RateUnit>("mb");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const epoch = useRef(0);
  const busy = useRef(false);

  function show(bps: string | null) {
    setLimited(bps !== null);
    if (bps !== null) {
      const split = splitRate(bps);
      setValue(split.value);
      setUnit(split.unit);
    }
  }

  useEffect(() => {
    const current = ++epoch.current;
    if (instance) {
      getSpeedLimit()
        .then((settings) => {
          if (current !== epoch.current) return;
          setLoaded({
            connection: service,
            revision: reload,
            bps: settings.global_bytes_per_second,
          });
          show(settings.global_bytes_per_second);
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
  }, [instance, service, getSpeedLimit, reload]);

  const ready = instance !== null && loaded?.connection === service && loaded.revision === reload;
  const bytes = limited ? toBytesPerSecond(value, unit) : null;
  const invalid = ready && limited && bytes === null;
  // An engine without the capability can still be set back to unlimited, never limited.
  const blockedByEngine = ready && limited && !supported;
  const saveReason = !ready
    ? t("speed.unavailable")
    : saving
      ? t("speed.saving")
      : blockedByEngine
        ? t("speed.engine")
        : invalid
          ? t("speed.invalid")
          : null;

  async function save() {
    if (saveReason || busy.current || !ready) return;
    const current = epoch.current;
    busy.current = true;
    setSaving(true);
    setSaved(false);
    setFailure(null);
    try {
      const settings = await setSpeedLimit(limited ? bytes : null);
      if (current !== epoch.current) return;
      setLoaded({ connection: service, revision: reload, bps: settings.global_bytes_per_second });
      show(settings.global_bytes_per_second);
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

  const disabled = !ready || saving;
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("speed.heading")}
      </h3>
      <form
        className="options-form"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="options-field options-field--inline">
          <input
            id={limitId}
            type="checkbox"
            checked={ready && limited}
            disabled={disabled || (!supported && !limited)}
            aria-describedby={noteId}
            onChange={(e) => {
              setLimited(e.target.checked);
              setSaved(false);
            }}
          />
          <label htmlFor={limitId}>{t("speed.limit")}</label>
        </div>
        {limited && ready && (
          <div className="options-field options-field--rate">
            <label htmlFor={valueId}>{t("speed.value")}</label>
            <input
              id={valueId}
              type="text"
              inputMode="decimal"
              value={value}
              disabled={disabled}
              aria-invalid={invalid || undefined}
              aria-describedby={`${noteId}${invalid ? ` ${errorId}` : ""}`}
              onChange={(e) => {
                setValue(e.target.value);
                setSaved(false);
              }}
            />
            <label htmlFor={unitId}>{t("speed.unit")}</label>
            <select
              id={unitId}
              value={unit}
              disabled={disabled}
              onChange={(e) => {
                setUnit(e.target.value as RateUnit);
                setSaved(false);
              }}
            >
              {RATE_UNITS.map((u) => (
                <option key={u} value={u}>
                  {t(`speed.unit.${u}`)}
                </option>
              ))}
            </select>
          </div>
        )}
        <p id={noteId} className="options-form__note">
          {t("speed.note")}
        </p>
        {ready && !supported && <p className="options-form__note">{t("speed.engine")}</p>}
        {invalid && (
          <p id={errorId} role="alert" className="options-form__note">
            {t("speed.invalid")}
          </p>
        )}
        {!ready && !loadFailed && (
          <p className="options-form__note">
            {t(instance ? "speed.loading" : "speed.unavailable")}
          </p>
        )}
        {failure && (
          <p role="alert" className="options-form__note">
            {failure.outcome === "uncertain"
              ? t("speed.uncertain")
              : failure.outcome === "rejected" &&
                  failure.error.kind === "BACKEND_CAPABILITY_UNAVAILABLE"
                ? t("speed.engine")
                : t("speed.failed")}
          </p>
        )}
        {instance && (loadFailed || failure?.outcome === "uncertain") && (
          <Button
            onClick={() => setReload((n) => n + 1)}
            {...(saving ? { unavailableReason: t("speed.saving") } : {})}
          >
            {t("speed.reload")}
          </Button>
        )}
        <Button type="submit" {...(saveReason ? { unavailableReason: saveReason } : {})}>
          {t(saving ? "speed.saving" : "speed.save")}
        </Button>
        {saved && ready && loaded && (
          <p role="status" aria-label={t("speed.status")} className="options-form__note">
            {loaded.bps === null
              ? t("speed.savedUnlimited")
              : format(t("speed.saved"), { rate: formatRate(loaded.bps) })}
          </p>
        )}
      </form>
    </section>
  );
}
