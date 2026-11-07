import { useCallback, useEffect, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import type { BrowserConnection, BrowserRepairOutcome } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import "./support.css";

type Load =
  { kind: "loading" } | { kind: "ready"; list: BrowserConnection[] } | { kind: "unavailable" };

const TONE = { supported: "success", untested: "info", unsupported: "warning" } as const;

/**
 * Help → Browser connections… (BRW-07/09, ADR 0029). Each browser found on this computer with how
 * it is installed (native, Snap, Flatpak or unknown), whether its connection points at this
 * installation, and the evidence-backed support verdict for that browser package, with Repair.
 * Repair never takes over another installation's or another program's entry; the shell reports
 * those. One repair per browser at a time; an answer from before "Check again" (or for a closed
 * dialog) is never shown.
 */
export function BrowserConnectionsDialog({ onClose }: { onClose: () => void }) {
  const { t } = usePreferences();
  const { browserConnections, repairBrowser } = useServiceActions();
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [results, setResults] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const generation = useRef(0);
  const inFlight = useRef(new Set<string>());

  // Answers are kept only for the generation that asked (a later "Check again" or closing wins).
  const fetchList = useCallback(
    (mine: number) =>
      browserConnections().then(
        (list) => mine === generation.current && setLoad({ kind: "ready", list }),
        () => mine === generation.current && setLoad({ kind: "unavailable" }),
      ),
    [browserConnections],
  );

  useEffect(() => {
    void fetchList(++generation.current);
    return () => {
      generation.current += 1;
    };
  }, [fetchList]);

  const refresh = () => {
    const mine = ++generation.current;
    inFlight.current.clear();
    setBusy({});
    setResults({});
    setLoad({ kind: "loading" });
    void fetchList(mine);
  };

  const resultText = (o: BrowserRepairOutcome): string => {
    if (o.outcome === "repaired") return t("browsers.repaired");
    if (o.outcome === "failed") return t("browsers.repairFailed");
    const key = `browsers.refused.${o.reason}` as MessageKey;
    return ["other_installation", "foreign", "unreadable"].includes(o.reason)
      ? t(key)
      : t("browsers.refused.other");
  };

  const repair = (browser: string) => {
    if (inFlight.current.has(browser)) return; // one request per browser, however fast the clicks
    inFlight.current.add(browser);
    const mine = generation.current;
    setBusy((b) => ({ ...b, [browser]: true }));
    repairBrowser(browser)
      .then(
        (o) => {
          if (mine !== generation.current) return;
          setResults((r) => ({ ...r, [browser]: resultText(o) }));
          if (o.outcome !== "failed")
            setLoad((l) =>
              l.kind === "ready"
                ? {
                    kind: "ready",
                    list: l.list.map((c) => (c.browser === browser ? o.connection : c)),
                  }
                : l,
            );
        },
        () =>
          mine === generation.current &&
          setResults((r) => ({ ...r, [browser]: t("browsers.repairFailed") })),
      )
      .finally(() => {
        if (mine !== generation.current) return;
        inFlight.current.delete(browser);
        setBusy((b) => ({ ...b, [browser]: false }));
      });
  };

  const shown =
    load.kind === "ready"
      ? load.list.filter((c) => c.packages.length > 0 || c.registration !== "absent")
      : [];
  const hidden = load.kind === "ready" && shown.length < load.list.length;

  return (
    <Dialog title={t("browsers.title")} onClose={onClose} closeLabel={t("options.close")}>
      <div className="browsers">
        <p>{t("browsers.intro")}</p>
        {load.kind === "loading" && <p role="status">{t("browsers.loading")}</p>}
        {load.kind === "unavailable" && (
          <Banner tone="warning" title={t("browsers.title")}>
            <p>{t("browsers.unavailable")}</p>
          </Banner>
        )}
        {load.kind === "ready" && (
          <ul className="browsers__list">
            {shown.map((c) => {
              const name = t(`browsers.name.${c.browser}` as MessageKey);
              return (
                <li key={c.browser} className="browsers__item" data-browser={c.browser}>
                  <h3 className="browsers__name">{name}</h3>
                  <dl className="browsers__facts">
                    <dt>{t("browsers.installedAs")}</dt>
                    <dd>
                      {c.packages.length > 0
                        ? c.packages.map((p) => t(`browsers.package.${p}` as MessageKey)).join(", ")
                        : t("browsers.package.none")}
                    </dd>
                    <dt>{t("browsers.connection")}</dt>
                    <dd data-registration={c.registration}>
                      {t(`browsers.registration.${c.registration}` as MessageKey)}
                    </dd>
                  </dl>
                  {c.support.map((v) => (
                    <Banner
                      key={v.package}
                      tone={TONE[v.support]}
                      title={`${t("browsers.support")}: ${t(`browsers.support.${v.support}` as MessageKey)}`}
                    >
                      <p data-support={v.support} data-reason={v.reason}>
                        {c.packages.length > 1 &&
                          `${t(`browsers.package.${v.package}` as MessageKey)}: `}
                        {t(`browsers.reason.${v.reason}` as MessageKey)}
                      </p>
                    </Banner>
                  ))}
                  {c.browser === "firefox" && (
                    <p className="browsers__note">{t("browsers.firefoxNote")}</p>
                  )}
                  <div className="browsers__actions">
                    <Button
                      onClick={() => repair(c.browser)}
                      aria-label={format(t("browsers.repairLabel"), { browser: name })}
                      aria-busy={busy[c.browser] === true}
                      {...(busy[c.browser] ? { unavailableReason: t("browsers.repairing") } : {})}
                    >
                      {t(busy[c.browser] ? "browsers.repairing" : "browsers.repair")}
                    </Button>
                    {results[c.browser] && <p role="status">{results[c.browser]}</p>}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        {hidden && <p className="browsers__note">{t("browsers.notFound")}</p>}
        <div className="browsers__actions">
          <Button onClick={refresh}>{t("browsers.checkAgain")}</Button>
        </div>
      </div>
    </Dialog>
  );
}
