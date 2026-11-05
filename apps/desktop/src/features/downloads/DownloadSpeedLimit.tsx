import { useId, useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
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
} from "../settings/speed";

/**
 * One download's own bandwidth limit (CORE-06), in its details. The value shown is the service's
 * (from the snapshot); a change applies to the running transfer without restarting it.
 */
export function DownloadSpeedLimit({ job }: { job: DownloadSnapshot }) {
  const { t } = usePreferences();
  const service = useServiceState();
  const { setDownloadSpeedLimit } = useServiceActions();
  const supported =
    service.status === "connected" && engineSupports(service.hello.backend, "speed_limit_per_job");
  const current = job.speed_limit_bytes_per_second ?? null;
  const headingId = useId();
  const valueId = useId();
  const unitId = useId();
  const errorId = useId();
  const initial = current ? splitRate(current) : { value: "", unit: "kb" as RateUnit };
  const [value, setValue] = useState(initial.value);
  const [unit, setUnit] = useState<RateUnit>(initial.unit);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);

  // A different download, or the service's value changed: show the authoritative value again
  // (adjusted while rendering, not in an effect).
  const shownKey = `${job.job_id}|${current ?? ""}`;
  const [shownFor, setShownFor] = useState(shownKey);
  if (shownFor !== shownKey) {
    setShownFor(shownKey);
    const next = current ? splitRate(current) : { value: "", unit: "kb" as RateUnit };
    setValue(next.value);
    setUnit(next.unit);
    setFailure(null);
  }

  const bytes = toBytesPerSecond(value, unit);
  const invalid = value.trim() !== "" && bytes === null;
  const connected = service.status === "connected";
  const setReason = !connected
    ? t("speed.unavailable")
    : !supported
      ? t("speed.engine")
      : pending
        ? t("speed.saving")
        : bytes === null
          ? t("speed.invalid")
          : bytes === current
            ? t("speed.unchanged")
            : null;

  async function apply(next: string | null) {
    if (pending) return;
    setPending(true);
    setFailure(null);
    try {
      await setDownloadSpeedLimit(job.job_id, next);
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="download-speed" role="group" aria-labelledby={headingId}>
      <p id={headingId} className="download-speed__heading">
        {current
          ? format(t("speed.job.current"), { rate: formatRate(current) })
          : t("speed.job.none")}
      </p>
      <div className="download-speed__fields">
        <label htmlFor={valueId}>{t("speed.job.value")}</label>
        <input
          id={valueId}
          type="text"
          inputMode="decimal"
          value={value}
          disabled={!connected || !supported || pending}
          aria-invalid={invalid || undefined}
          aria-describedby={invalid ? errorId : undefined}
          onChange={(e) => setValue(e.target.value)}
        />
        <label htmlFor={unitId}>{t("speed.unit")}</label>
        <select
          id={unitId}
          value={unit}
          disabled={!connected || !supported || pending}
          onChange={(e) => setUnit(e.target.value as RateUnit)}
        >
          {RATE_UNITS.map((u) => (
            <option key={u} value={u}>
              {t(`speed.unit.${u}`)}
            </option>
          ))}
        </select>
        <Button
          onClick={() => void apply(bytes)}
          {...(setReason ? { unavailableReason: setReason } : {})}
        >
          {pending ? t("speed.saving") : t("speed.job.set")}
        </Button>
        {current && (
          <Button
            onClick={() => void apply(null)}
            {...(pending
              ? { unavailableReason: t("speed.saving") }
              : !connected
                ? { unavailableReason: t("speed.unavailable") }
                : {})}
          >
            {t("speed.job.remove")}
          </Button>
        )}
      </div>
      {invalid && (
        <p id={errorId} role="alert" className="download-speed__note">
          {t("speed.invalid")}
        </p>
      )}
      {connected && !supported && <p className="download-speed__note">{t("speed.engine")}</p>}
      {failure && (
        <p role="alert" className="download-speed__note">
          {failure.outcome === "uncertain" ? t("speed.job.uncertain") : t("speed.failed")}
        </p>
      )}
    </div>
  );
}
