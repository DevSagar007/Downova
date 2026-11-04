import { useEffect, useId, useRef, useState } from "react";
import type { DownloadSnapshot, QueueInfo } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useDownloads, useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";

/** The optional aria2 engine's own active-file cap (ADR 0023): the service never admits more. */
export const ARIA2_ACTIVE_CAP = 3;
const MAIN_QUEUE = "main";
/** Phases in which the service counts a download as using a slot (`Job::is_active`). */
const ACTIVE_PHASES = new Set(["probing", "transferring", "scanning", "finalizing"]);
const HELD = new Set(["queue_limit", "queue_stopped", "disk_unavailable", "volume_missing"]);

/** Whether `d` uses an active-download slot right now, as the service counts it. */
export function usesSlot(d: DownloadSnapshot): boolean {
  return (
    d.desired_state === "running" &&
    d.attempt_id !== null &&
    ACTIVE_PHASES.has(d.phase) &&
    !d.blockers.some((b) => HELD.has(b))
  );
}

/** What limits the Main queue (B2.31): every input is the service's own state. */
export function mainCapacity(global: number, main: QueueInfo, aria2: boolean) {
  const byLimits = Math.min(global, main.max_active);
  const effective = aria2 ? Math.min(byLimits, ARIA2_ACTIVE_CAP) : byLimits;
  return {
    effective,
    /** The queue's own limit is what stops Main from running more. */
    queueRestricts: main.max_active < global,
    /** Both limits allow more, but the aria2 engine does not. */
    engineRestricts: aria2 && ARIA2_ACTIVE_CAP < byLimits,
  };
}

/** Global admission is service-owned; this control never silently changes a queue. */
export function ConcurrencySettings({
  onManageQueues,
}: { onManageQueues?: (() => void) | undefined } = {}) {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getConcurrency, setConcurrency, reloadQueues } = useServiceActions();
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
      // The queue limits come from the service too, read afresh with the global limit.
      void reloadQueues();
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
  }, [instance, service, getConcurrency, reloadQueues, reload]);

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
      {ready && loaded && (
        <LimitsExplanation global={loaded.limit} onManageQueues={onManageQueues} />
      )}
    </section>
  );
}

/** The saved limits and what they allow for Main, from the service's state (never the draft). */
function LimitsExplanation({
  global,
  onManageQueues,
}: {
  global: number;
  onManageQueues?: (() => void) | undefined;
}) {
  const { t } = usePreferences();
  const service = useServiceState();
  const { queues, items } = useDownloads();
  const summaryId = useId();
  const main = queues.find((q) => q.queue_id === MAIN_QUEUE);
  const aria2 = service.status === "connected" && service.hello.backend.id === "aria2";
  const active = items.filter(usesSlot);
  const activeMain = active.filter((d) => (d.queue_id ?? MAIN_QUEUE) === MAIN_QUEUE).length;
  const capacity = main ? mainCapacity(global, main, aria2) : null;
  return (
    <div className="options-limits" role="group" aria-labelledby={summaryId}>
      <h4 id={summaryId} className="options-limits__heading">
        {t("concurrency.summary.heading")}
      </h4>
      <dl className="options-limits__values">
        <div>
          <dt>{t("concurrency.summary.global")}</dt>
          <dd>{global}</dd>
        </div>
        <div>
          <dt>{t("concurrency.summary.main")}</dt>
          <dd>{main ? main.max_active : t("concurrency.summary.unknown")}</dd>
        </div>
        {capacity && (
          <div>
            <dt>{t("concurrency.summary.effective")}</dt>
            <dd>{capacity.effective}</dd>
          </div>
        )}
        <div>
          <dt>{t("concurrency.summary.activeNow")}</dt>
          <dd>
            {format(t("concurrency.summary.activeCounts"), {
              all: active.length,
              main: activeMain,
            })}
          </dd>
        </div>
      </dl>
      {capacity && main && (
        <>
          <p className="options-form__note">
            {format(t("concurrency.summary.allows"), { n: capacity.effective })}
          </p>
          {capacity.queueRestricts && (
            <p className="options-form__note">
              {format(t("concurrency.summary.queueRestricts"), { main: main.max_active })}
            </p>
          )}
          {capacity.engineRestricts && (
            <p className="options-form__note">
              {format(t("concurrency.summary.aria2"), {
                cap: ARIA2_ACTIVE_CAP,
                n: capacity.effective,
              })}
            </p>
          )}
          {!main.running && (
            <p className="options-form__note">{t("concurrency.summary.stopped")}</p>
          )}
          {capacity.queueRestricts && onManageQueues && (
            <div className="options-actions">
              <Button onClick={onManageQueues}>{t("concurrency.summary.changeQueue")}</Button>
            </div>
          )}
        </>
      )}
      <p className="options-form__note">{t("concurrency.summary.shared")}</p>
      {active.length > global && (
        <p className="options-form__note">{t("concurrency.summary.over")}</p>
      )}
    </div>
  );
}
