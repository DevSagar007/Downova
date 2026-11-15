import { useEffect, useId, useRef, useState } from "react";
import type { KnownHost } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { Button } from "../../ui/Button";

/**
 * Options → Known servers (F05, ADR 0037): the SFTP host keys this computer trusts, and keys
 * downloads were shown but not trusted. Trusting happens from the stopped download (where the key
 * is shown in context); here a server can be forgotten, for example after an expected key change.
 */
export function KnownServersSettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { listKnownHosts, removeKnownHost } = useServiceActions();
  const instance = service.status === "connected" ? service.hello.service_instance_id : null;
  const headingId = useId();
  const [hosts, setHosts] = useState<KnownHost[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const epoch = useRef(0);

  useEffect(() => {
    const current = ++epoch.current;
    if (instance) {
      listKnownHosts()
        .then((r) => {
          if (current === epoch.current) setHosts(r.hosts);
        })
        .catch(() => {
          if (current === epoch.current) setFailed(true);
        });
    }
    return () => {
      epoch.current = current + 1;
    };
  }, [instance, listKnownHosts]);

  async function forget(h: KnownHost) {
    if (busy) return;
    setBusy(true);
    setStatus(null);
    setFailed(false);
    try {
      const r = await removeKnownHost(h.host, h.port);
      setHosts(r.hosts);
      setStatus(format(t("knownHosts.removed"), { server: `${h.host}:${h.port}` }));
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }

  const trusted = hosts?.filter((h) => h.state === "trusted") ?? [];
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("knownHosts.heading")}
      </h3>
      <p className="options-form__note">{t("knownHosts.note")}</p>
      {hosts === null && !failed && (
        <p className="options-form__note">
          {t(instance ? "knownHosts.loading" : "knownHosts.unavailable")}
        </p>
      )}
      {hosts !== null && trusted.length === 0 && (
        <p className="options-form__note">{t("knownHosts.empty")}</p>
      )}
      {trusted.length > 0 && (
        <ul className="site-logins" aria-label={t("knownHosts.list")}>
          {trusted.map((h) => (
            <li key={`${h.host}:${h.port}`}>
              <span dir="ltr">
                sftp://{h.host}:{h.port}
              </span>{" "}
              —{" "}
              <code dir="ltr" className="download-details__hash">
                {h.algorithm} {h.fingerprint}
              </code>{" "}
              <Button
                onClick={() => void forget(h)}
                aria-label={format(t("knownHosts.remove"), { server: `${h.host}:${h.port}` })}
                {...(busy ? { unavailableReason: t("knownHosts.removing") } : {})}
              >
                {t("siteLogins.removeShort")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {failed && (
        <p role="alert" className="options-form__note">
          {t("knownHosts.failed")}
        </p>
      )}
      {status && (
        <p role="status" aria-label={t("knownHosts.status")} className="options-form__note">
          {status}
        </p>
      )}
    </section>
  );
}
