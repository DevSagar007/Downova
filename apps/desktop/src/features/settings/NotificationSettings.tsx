import { useEffect, useId, useRef, useState } from "react";
import type { NotificationStatus } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";

/**
 * Options → Notifications (CORE-09, ADR 0022). The background service shows them — also while
 * the window is hidden or closed — when a download completes (after it was checked and placed)
 * or fails. Off by default; turning it on is the explicit action that shows a first confirmation
 * (where the system asks for permission, it asks then). A blocked system is reported and never
 * asked again. Notifications never open a downloaded file.
 */
export function NotificationSettings() {
  const { t, locale } = usePreferences();
  const service = useServiceState();
  const { getNotifications, setNotifications } = useServiceActions();
  const headingId = useId();
  const checkboxId = useId();
  const noteId = useId();
  const connected = service.status === "connected";
  const [status, setStatus] = useState<NotificationStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const busy = useRef(false);

  useEffect(() => {
    if (!connected) return;
    let active = true;
    getNotifications()
      .then((s) => active && setStatus(s))
      .catch(() => active && setStatus(null));
    return () => {
      active = false;
    };
  }, [connected, getNotifications]);

  async function toggle(enabled: boolean) {
    if (busy.current) return;
    busy.current = true;
    setSaving(true);
    setFailure(null);
    try {
      setStatus(await setNotifications(enabled, locale));
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      busy.current = false;
      setSaving(false);
    }
  }

  const unavailable = status?.availability === "unavailable";
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("notifications.heading")}
      </h3>
      {!connected || !status ? (
        <p className="options-form__note">{t("notifications.serviceUnavailable")}</p>
      ) : (
        <>
          <div className="options-check">
            <input
              id={checkboxId}
              type="checkbox"
              checked={status.enabled}
              aria-describedby={noteId}
              aria-disabled={saving || (unavailable && !status.enabled) || undefined}
              onChange={(e) => {
                if (saving || (unavailable && !status.enabled)) return;
                void toggle(e.target.checked);
              }}
            />
            <label htmlFor={checkboxId}>{t("notifications.enable")}</label>
          </div>
          <p id={noteId} className="options-form__note">
            {unavailable ? t("notifications.unavailable") : t("notifications.note")}
          </p>
          {status.enabled && status.last_delivery_failed && (
            <p role="status" className="options-form__note">
              {t("notifications.blocked")}
            </p>
          )}
          {failure && (
            <p role="alert" className="options-form__note">
              {t("notifications.failed")}{" "}
              {failure.outcome === "rejected"
                ? t(`error.kind.${failure.error.kind}`)
                : t("command.uncertain")}
            </p>
          )}
        </>
      )}
    </section>
  );
}
