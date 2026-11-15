import { useEffect, useId, useRef, useState } from "react";
import type { SiteLogin } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { proxyProblem } from "./ProxySettings";

/**
 * Options → Site logins (F05, ADR 0036): one user name and password per FTP server, used for every
 * download from it. The service owns the list; a password goes only to the OS credential store,
 * is never shown or read back, and this form holds a typed one only until it is sent.
 */
export function SiteLoginsSettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { listSiteLogins, setSiteLogin, removeSiteLogin } = useServiceActions();
  const instance = service.status === "connected" ? service.hello.service_instance_id : null;
  const headingId = useId();
  const hostId = useId();
  const portId = useId();
  const userId = useId();
  const passwordId = useId();
  const noteId = useId();
  const [logins, setLogins] = useState<SiteLogin[] | null>(null);
  const [host, setHost] = useState("");
  const [port, setPort] = useState("21");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const epoch = useRef(0);

  useEffect(() => {
    const current = ++epoch.current;
    if (instance) {
      listSiteLogins()
        .then((r) => {
          if (current === epoch.current) setLogins(r.logins);
        })
        .catch((thrown: unknown) => {
          if (current !== epoch.current) return;
          setLogins(null);
          setFailure(toCallError(thrown));
        });
    }
    return () => {
      epoch.current = current + 1;
    };
  }, [instance, listSiteLogins]);

  const ready = instance !== null && logins !== null;
  const trimmedHost = host.trim().toLowerCase();
  const portNumber = Number(port.trim());
  const existing = logins?.find((l) => l.host === trimmedHost && l.port === portNumber);
  const shape = proxyProblem({ mode: "http", host, port, username, password });
  const problem =
    shape ??
    (username.trim() === ""
      ? "siteLogins.invalid.username"
      : password === "" && !existing
        ? "siteLogins.invalid.password"
        : null);
  const saveReason = !ready
    ? t("siteLogins.unavailable")
    : busy
      ? t("siteLogins.saving")
      : problem
        ? t(problem)
        : null;

  async function save() {
    if (saveReason || busy) return;
    setBusy(true);
    setStatus(null);
    setFailure(null);
    try {
      const r = await setSiteLogin({
        scheme: "ftp",
        host: trimmedHost,
        port: portNumber,
        username: username.trim(),
        ...(password ? { password } : {}),
      });
      setLogins(r.logins);
      setStatus(format(t("siteLogins.saved"), { server: `${trimmedHost}:${portNumber}` }));
      setHost("");
      setPort("21");
      setUsername("");
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      // A typed password is never kept after a send, whatever its outcome.
      setPassword("");
      setBusy(false);
    }
  }

  async function remove(login: SiteLogin) {
    if (busy) return;
    setBusy(true);
    setStatus(null);
    setFailure(null);
    try {
      const r = await removeSiteLogin(login.login_id);
      setLogins(r.logins);
      setStatus(format(t("siteLogins.removed"), { server: `${login.host}:${login.port}` }));
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("siteLogins.heading")}
      </h3>
      <p id={noteId} className="options-form__note">
        {t("siteLogins.note")}
      </p>
      {ready && logins.length === 0 && (
        <p className="options-form__note">{t("siteLogins.empty")}</p>
      )}
      {ready && logins.length > 0 && (
        <ul className="site-logins" aria-label={t("siteLogins.list")}>
          {logins.map((l) => (
            <li key={l.login_id}>
              <span dir="ltr">
                ftp://{l.host}:{l.port}
              </span>{" "}
              — {format(t("siteLogins.user"), { user: l.username })} —{" "}
              {t(
                l.password_state === "saved"
                  ? "siteLogins.password.saved"
                  : "siteLogins.password.unavailable",
              )}{" "}
              <Button
                onClick={() => void remove(l)}
                aria-label={format(t("siteLogins.remove"), { server: `${l.host}:${l.port}` })}
                {...(busy ? { unavailableReason: t("siteLogins.saving") } : {})}
              >
                {t("siteLogins.removeShort")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="options-form"
        noValidate
        autoComplete="off"
        aria-describedby={noteId}
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="options-field">
          <label htmlFor={hostId}>{t("siteLogins.host")}</label>
          <input
            id={hostId}
            type="text"
            dir="ltr"
            spellCheck={false}
            value={host}
            disabled={!ready || busy}
            onChange={(e) => setHost(e.target.value)}
          />
        </div>
        <div className="options-field">
          <label htmlFor={portId}>{t("proxy.port")}</label>
          <input
            id={portId}
            type="text"
            inputMode="numeric"
            value={port}
            disabled={!ready || busy}
            onChange={(e) => setPort(e.target.value)}
          />
        </div>
        <div className="options-field">
          <label htmlFor={userId}>{t("siteLogins.username")}</label>
          <input
            id={userId}
            type="text"
            spellCheck={false}
            autoComplete="off"
            value={username}
            disabled={!ready || busy}
            onChange={(e) => setUsername(e.target.value)}
          />
        </div>
        <div className="options-field">
          <label htmlFor={passwordId}>{t("proxy.password")}</label>
          <input
            id={passwordId}
            type="password"
            autoComplete="new-password"
            value={password}
            disabled={!ready || busy}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        {existing && password === "" && (
          <p className="options-form__note">{t("siteLogins.keepPassword")}</p>
        )}
        {ready && problem && (host || username || password) && (
          <p role="alert" className="options-form__note">
            {t(problem)}
          </p>
        )}
        {!ready && !failure && (
          <p className="options-form__note">
            {t(instance ? "siteLogins.loading" : "siteLogins.unavailable")}
          </p>
        )}
        {failure && (
          <p role="alert" className="options-form__note">
            {failure.outcome === "rejected" && failure.error.kind === "CREDENTIAL_STORE_UNAVAILABLE"
              ? t("siteLogins.storeFailed")
              : failure.outcome === "uncertain"
                ? t("siteLogins.uncertain")
                : t("siteLogins.failed")}
          </p>
        )}
        <Button type="submit" {...(saveReason ? { unavailableReason: saveReason } : {})}>
          {t(busy ? "siteLogins.saving" : "siteLogins.save")}
        </Button>
        {status && (
          <p role="status" aria-label={t("siteLogins.status")} className="options-form__note">
            {status}
          </p>
        )}
      </form>
    </section>
  );
}
