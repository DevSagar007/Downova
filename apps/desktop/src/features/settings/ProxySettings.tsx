import { useEffect, useId, useRef, useState } from "react";
import type { ProxyMode, ProxySettings as Saved, SetProxyParams } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions, useServiceState } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { engineSupports } from "./speed";

/** The schema's host rule: a host name, an IPv4 address or a bracketed IPv6 address. */
const HOST =
  /^([A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)*|\[[0-9A-Fa-f:.]+\])$/;

/** Validation of the draft; `null` when it can be saved. */
export function proxyProblem(draft: {
  mode: ProxyMode;
  host: string;
  port: string;
  username: string;
  password: string;
}):
  | "proxy.invalid.host"
  | "proxy.invalid.port"
  | "proxy.invalid.username"
  | "proxy.invalid.passwordNeedsUser"
  | null {
  const host = draft.host.trim();
  const port = draft.port.trim();
  const needed = draft.mode === "http";
  if ((needed || host) && (!HOST.test(host) || host.length > 253)) return "proxy.invalid.host";
  if (
    (needed || port) &&
    !(/^[0-9]{1,5}$/.test(port) && Number(port) >= 1 && Number(port) <= 65535)
  ) {
    return "proxy.invalid.port";
  }
  if (draft.username.includes(":")) return "proxy.invalid.username";
  if (draft.password && !draft.username.trim()) return "proxy.invalid.passwordNeedsUser";
  return null;
}

/**
 * Options → Proxy (F06, ADR 0034). The service owns the setting; the password goes only to the
 * OS credential store and is never shown or read back: this form holds a typed password until it
 * is sent, then clears it.
 */
export function ProxySettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getProxy, setProxy } = useServiceActions();
  const instance = service.status === "connected" ? service.hello.service_instance_id : null;
  const supported =
    service.status === "connected" && engineSupports(service.hello.backend, "http_proxy");
  const headingId = useId();
  const legendId = useId();
  const hostId = useId();
  const portId = useId();
  const userId = useId();
  const passwordId = useId();
  const removeId = useId();
  const noteId = useId();
  const errorId = useId();
  const [loaded, setLoaded] = useState<{
    connection: ReturnType<typeof useServiceState>;
    revision: number;
    settings: Saved;
  } | null>(null);
  const [mode, setMode] = useState<ProxyMode>("direct");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [removePassword, setRemovePassword] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const epoch = useRef(0);
  const busy = useRef(false);

  function show(s: Saved) {
    setMode(s.mode);
    setHost(s.host ?? "");
    setPort(s.port === null ? "" : String(s.port));
    setUsername(s.username ?? "");
    setPassword("");
    setRemovePassword(false);
  }

  useEffect(() => {
    const current = ++epoch.current;
    if (instance) {
      getProxy()
        .then((settings) => {
          if (current !== epoch.current) return;
          setLoaded({ connection: service, revision: reload, settings });
          show(settings);
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
  }, [instance, service, getProxy, reload]);

  const ready = instance !== null && loaded?.connection === service && loaded.revision === reload;
  const problem = ready ? proxyProblem({ mode, host, port, username, password }) : null;
  const blockedByEngine = ready && mode === "http" && !supported;
  const saveReason = !ready
    ? t("proxy.unavailable")
    : saving
      ? t("proxy.saving")
      : blockedByEngine
        ? t("proxy.engine")
        : problem
          ? t(problem)
          : null;
  const savedState = loaded?.settings.password_state ?? "none";

  async function save() {
    if (saveReason || busy.current || !ready) return;
    const current = epoch.current;
    busy.current = true;
    setSaving(true);
    setSaved(false);
    setFailure(null);
    const user = username.trim();
    const params: SetProxyParams = {
      mode,
      host: host.trim() || null,
      port: port.trim() ? Number(port.trim()) : null,
      username: user || null,
    };
    if (password) params.password = password;
    else if (removePassword) params.password = null;
    try {
      const settings = await setProxy(params);
      if (current !== epoch.current) return;
      setLoaded({ connection: service, revision: reload, settings });
      show(settings);
      setSaved(true);
    } catch (thrown) {
      if (current === epoch.current) {
        setFailure(toCallError(thrown, true));
        // The typed password is not kept after a send, whatever its outcome.
        setPassword("");
      }
    } finally {
      if (current === epoch.current) {
        busy.current = false;
        setSaving(false);
      }
    }
  }

  const disabled = !ready || saving;
  const carried = loaded?.settings.carries ?? [];
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("proxy.heading")}
      </h3>
      <form
        className="options-form"
        noValidate
        autoComplete="off"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <fieldset className="options-fieldset" aria-describedby={noteId}>
          <legend id={legendId}>{t("proxy.mode")}</legend>
          {(["direct", "http"] as const).map((value) => (
            <div key={value} className="options-field options-field--inline">
              <input
                id={`${legendId}-${value}`}
                type="radio"
                name={legendId}
                value={value}
                checked={ready && mode === value}
                disabled={disabled || (value === "http" && !supported && mode !== "http")}
                onChange={() => {
                  setMode(value);
                  setSaved(false);
                }}
              />
              <label htmlFor={`${legendId}-${value}`}>{t(`proxy.mode.${value}`)}</label>
            </div>
          ))}
        </fieldset>
        {ready && (
          <>
            <div className="options-field">
              <label htmlFor={hostId}>{t("proxy.host")}</label>
              <input
                id={hostId}
                type="text"
                spellCheck={false}
                value={host}
                disabled={disabled}
                aria-invalid={problem === "proxy.invalid.host" || undefined}
                aria-describedby={problem === "proxy.invalid.host" ? errorId : undefined}
                onChange={(e) => {
                  setHost(e.target.value);
                  setSaved(false);
                }}
              />
            </div>
            <div className="options-field">
              <label htmlFor={portId}>{t("proxy.port")}</label>
              <input
                id={portId}
                type="text"
                inputMode="numeric"
                value={port}
                disabled={disabled}
                aria-invalid={problem === "proxy.invalid.port" || undefined}
                aria-describedby={problem === "proxy.invalid.port" ? errorId : undefined}
                onChange={(e) => {
                  setPort(e.target.value);
                  setSaved(false);
                }}
              />
            </div>
            <div className="options-field">
              <label htmlFor={userId}>{t("proxy.username")}</label>
              <input
                id={userId}
                type="text"
                spellCheck={false}
                autoComplete="off"
                value={username}
                disabled={disabled}
                aria-invalid={problem === "proxy.invalid.username" || undefined}
                onChange={(e) => {
                  setUsername(e.target.value);
                  setSaved(false);
                }}
              />
            </div>
            <div className="options-field">
              <label htmlFor={passwordId}>{t("proxy.password")}</label>
              <input
                id={passwordId}
                type="password"
                autoComplete="new-password"
                value={password}
                disabled={disabled || removePassword}
                aria-invalid={problem === "proxy.invalid.passwordNeedsUser" || undefined}
                onChange={(e) => {
                  setPassword(e.target.value);
                  setSaved(false);
                }}
              />
            </div>
            {savedState !== "none" && (
              <>
                <p
                  className="options-form__note"
                  role={savedState === "unavailable" ? "alert" : undefined}
                >
                  {t(
                    savedState === "unavailable"
                      ? "proxy.password.unavailable"
                      : "proxy.password.saved",
                  )}
                </p>
                <div className="options-field options-field--inline">
                  <input
                    id={removeId}
                    type="checkbox"
                    checked={removePassword}
                    disabled={disabled || password !== ""}
                    onChange={(e) => {
                      setRemovePassword(e.target.checked);
                      setSaved(false);
                    }}
                  />
                  <label htmlFor={removeId}>{t("proxy.password.remove")}</label>
                </div>
              </>
            )}
          </>
        )}
        <p id={noteId} className="options-form__note">
          {t("proxy.note")}
        </p>
        {ready && loaded?.settings.mode === "http" && carried.length > 0 && (
          <p className="options-form__note">
            {format(t("proxy.carries"), {
              protocols: carried.map((c) => c.toUpperCase()).join(", "),
            })}
          </p>
        )}
        {ready && !supported && <p className="options-form__note">{t("proxy.engine")}</p>}
        {problem && (
          <p id={errorId} role="alert" className="options-form__note">
            {t(problem)}
          </p>
        )}
        {!ready && !loadFailed && (
          <p className="options-form__note">
            {t(instance ? "proxy.loading" : "proxy.unavailable")}
          </p>
        )}
        {failure && (
          <p role="alert" className="options-form__note">
            {failure.outcome === "uncertain"
              ? t("proxy.uncertain")
              : failure.outcome === "rejected" &&
                  failure.error.kind === "BACKEND_CAPABILITY_UNAVAILABLE"
                ? t("proxy.engine")
                : failure.outcome === "rejected" &&
                    failure.error.kind === "CREDENTIAL_STORE_UNAVAILABLE"
                  ? t("proxy.storeFailed")
                  : t("proxy.failed")}
          </p>
        )}
        {instance && (loadFailed || failure?.outcome === "uncertain") && (
          <Button
            onClick={() => setReload((n) => n + 1)}
            {...(saving ? { unavailableReason: t("proxy.saving") } : {})}
          >
            {t("proxy.reload")}
          </Button>
        )}
        <Button type="submit" {...(saveReason ? { unavailableReason: saveReason } : {})}>
          {t(saving ? "proxy.saving" : "proxy.save")}
        </Button>
        {saved && ready && loaded && (
          <p role="status" aria-label={t("proxy.status")} className="options-form__note">
            {loaded.settings.mode === "http"
              ? format(t("proxy.saved.http"), {
                  host: loaded.settings.host ?? "",
                  port: String(loaded.settings.port ?? ""),
                })
              : t("proxy.saved.direct")}
          </p>
        )}
      </form>
    </section>
  );
}
