import { useEffect, useId, useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { Banner } from "../../ui/Banner";
import { Dialog } from "../../ui/Dialog";
import "./support.css";

/** One redistributed component (REL-03 inventory, `scripts/notices/generate-notices.mjs`). */
export interface NoticeComponent {
  name: string;
  version: string | null;
  ecosystem: string;
  license: string | null;
  source: string;
  repository: string | null;
  targets: string[];
  texts: string[];
  unresolved: string[];
}

export interface NoticeData {
  summary: { distributed: number; unresolved: number };
  components: NoticeComponent[];
  external: { name: string; version: string | null; reason: string }[];
  texts: Record<string, { file: string; text: string }>;
}

function isNoticeData(v: unknown): v is NoticeData {
  const d = v as NoticeData | null;
  return (
    !!d &&
    Array.isArray(d.components) &&
    Array.isArray(d.external) &&
    typeof d.texts === "object" &&
    typeof d.summary?.distributed === "number"
  );
}

/** Loads the inventory bundled with this build (no network: it is part of the app's files). */
export const loadNotices = async (): Promise<NoticeData> => {
  const mod = await import("../../generated/third-party.notices.json");
  if (!isNoticeData(mod.default)) throw new Error("unreadable notices");
  return mod.default;
};

/**
 * Help → Third-party notices… (REL-03). Offline: the inventory and license texts are bundled
 * with the app. The surrounding text follows the UI language; component names, versions,
 * license identifiers and license texts are shown exactly as published (English, left to right).
 */
export function NoticesDialog({
  onClose,
  load = loadNotices,
}: {
  onClose: () => void;
  load?: () => Promise<NoticeData>;
}) {
  const { t } = usePreferences();
  const [data, setData] = useState<NoticeData | null>(null);
  const [failed, setFailed] = useState(false);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const filterId = useId();

  useEffect(() => {
    let live = true;
    load().then(
      (d) => live && setData(d),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, [load]);

  const shown = useMemo(() => {
    if (!data) return [];
    const q = filter.trim().toLowerCase();
    if (!q) return data.components;
    return data.components.filter((c) =>
      `${c.name} ${c.version ?? ""} ${c.license ?? ""}`.toLowerCase().includes(q),
    );
  }, [data, filter]);

  return (
    <Dialog title={t("notices.title")} closeLabel={t("options.close")} onClose={onClose}>
      <div className="notices">
        <p>{t("notices.intro")}</p>
        {failed && <Banner tone="danger" title={t("notices.loadFailed")} />}
        {!data && !failed && <p role="status">{t("notices.loading")}</p>}
        {data && (
          <>
            <p className="notices__summary">
              {format(t("notices.summary"), {
                count: data.summary.distributed,
                unresolved: data.summary.unresolved,
              })}
            </p>
            <p className="notices__summary">{t("notices.original")}</p>
            <label htmlFor={filterId} className="notices__filter">
              {t("notices.filter")}
              <input
                id={filterId}
                type="search"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
            </label>
            {shown.length === 0 && <p role="status">{t("notices.none")}</p>}
            <ul className="notices__list" aria-label={t("notices.listLabel")}>
              {shown.map((c) => {
                const key = `${c.ecosystem}:${c.name}@${c.version ?? ""}`;
                const expanded = open === key;
                const panelId = `notice-${key.replace(/[^a-z0-9]/gi, "-")}`;
                return (
                  <li key={key} className="notices__item">
                    <button
                      type="button"
                      className="notices__toggle"
                      aria-expanded={expanded}
                      aria-controls={panelId}
                      onClick={() => setOpen(expanded ? null : key)}
                    >
                      {expanded ? (
                        <ChevronDown className="icon" aria-hidden="true" />
                      ) : (
                        <ChevronRight className="icon" aria-hidden="true" />
                      )}
                      <span lang="en" dir="ltr" className="notices__name">
                        {c.name} {c.version ?? t("notices.unknownVersion")}
                      </span>
                      <span className="visually-hidden">, </span>
                      <span lang="en" dir="ltr" className="notices__license">
                        {c.license ?? t("notices.unknownLicense")}
                      </span>
                    </button>
                    {expanded && (
                      <div id={panelId} className="notices__detail">
                        <dl className="notices__facts">
                          <dt>{t("notices.source")}</dt>
                          <dd lang="en" dir="ltr">
                            {c.source}
                          </dd>
                          {c.repository && (
                            <>
                              <dt>{t("notices.repository")}</dt>
                              <dd lang="en" dir="ltr">
                                {c.repository}
                              </dd>
                            </>
                          )}
                          <dt>{t("notices.targets")}</dt>
                          <dd lang="en" dir="ltr">
                            {c.targets.join(", ")}
                          </dd>
                        </dl>
                        {c.unresolved.length > 0 && (
                          <Banner tone="warning" title={t("notices.unresolved")} live="off">
                            <ul lang="en" dir="ltr">
                              {c.unresolved.map((u) => (
                                <li key={u}>{u}</li>
                              ))}
                            </ul>
                          </Banner>
                        )}
                        {c.texts.length === 0 && <p>{t("notices.noText")}</p>}
                        {c.texts.map((id) => (
                          <figure key={id} className="notices__text">
                            <figcaption lang="en" dir="ltr">
                              {data.texts[id]?.file}
                            </figcaption>
                            <textarea
                              lang="en"
                              dir="ltr"
                              readOnly
                              rows={12}
                              aria-label={data.texts[id]?.file}
                              value={data.texts[id]?.text ?? ""}
                            />
                          </figure>
                        ))}
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
            <h3 className="notices__heading">{t("notices.external")}</h3>
            <ul className="notices__external">
              {data.external.map((e) => (
                <li key={e.name} lang="en" dir="ltr">
                  <strong>
                    {e.name}
                    {e.version ? ` ${e.version}` : ""}
                  </strong>
                  : {e.reason}
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </Dialog>
  );
}
