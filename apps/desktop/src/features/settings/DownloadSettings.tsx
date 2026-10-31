import { useEffect, useId, useRef, useState } from "react";
import type { CollisionPolicy, FileCategory, ServiceSettings } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { useServiceActions, useServiceState, useShell } from "../../services/ServiceProvider";
import { format } from "../../i18n/messages";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { FILE_CATEGORIES } from "../downloads/categories";

const POLICIES: CollisionPolicy[] = ["rename", "ask"];

type Saved = { kind: "saved" } | { kind: "failed"; failure: ServiceCallError } | null;

/**
 * Options → Downloads (CORE-07, ADR 0015): the folder for each file category and what happens
 * when a final name is taken. Saved by the service (`settings.v1.set`), which checks every
 * folder; the window only edits text. A change applies to new downloads only.
 *
 * Browse… (B2.19) opens the native folder dialog after a click and only fills the field: the
 * choice is saved, like typed text, with Save. Cancelling the dialog changes nothing.
 */
export function DownloadSettings() {
  const { t } = usePreferences();
  const service = useServiceState();
  const { getSettings, setSettings } = useServiceActions();
  const shell = useShell();
  const [browseFailed, setBrowseFailed] = useState(false);
  const browsing = useRef(false);
  const headingId = useId();
  const collisionId = useId();
  const noteId = useId();
  const fieldId = useId();
  const connected = service.status === "connected";
  const [folders, setFolders] = useState<Record<FileCategory, string> | null>(null);
  const [policy, setPolicy] = useState<CollisionPolicy>("rename");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<Saved>(null);

  useEffect(() => {
    if (!connected) return;
    let active = true;
    getSettings()
      .then((s) => {
        if (!active) return;
        const next = Object.fromEntries(FILE_CATEGORIES.map((c) => [c, ""])) as Record<
          FileCategory,
          string
        >;
        for (const rule of s.destination_rules) next[rule.category] = rule.directory;
        setFolders(next);
        setPolicy(s.on_collision);
      })
      .catch(() => {
        if (active) setFolders(null);
      });
    return () => {
      active = false;
    };
  }, [connected, getSettings]);

  async function browse(c: FileCategory) {
    if (!folders || browsing.current) return;
    browsing.current = true;
    setBrowseFailed(false);
    try {
      const start = folders[c].trim();
      const chosen = await shell.pickFolder(
        start === "" ? null : start,
        format(t("options.browseFor"), { category: t(`categories.${c}`) }),
      );
      // null: the user cancelled — the field and the saved settings stay as they are.
      if (chosen !== null) {
        setSaved(null);
        setFolders((current) => (current ? { ...current, [c]: chosen } : current));
      }
    } catch {
      setBrowseFailed(true);
    } finally {
      browsing.current = false;
    }
  }

  async function save() {
    if (!folders || saving) return;
    setSaving(true);
    setSaved(null);
    const settings: ServiceSettings = {
      destination_rules: FILE_CATEGORIES.filter((c) => folders[c].trim() !== "").map((c) => ({
        category: c,
        directory: folders[c].trim(),
      })) as ServiceSettings["destination_rules"],
      on_collision: policy,
    };
    try {
      await setSettings(settings);
      setSaved({ kind: "saved" });
    } catch (thrown) {
      setSaved({ kind: "failed", failure: toCallError(thrown, true) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("options.downloads")}
      </h3>
      {!connected || !folders ? (
        <p className="options-form__note">{t("options.service.unavailable")}</p>
      ) : (
        <>
          <fieldset className="options-fieldset" aria-describedby={noteId}>
            <legend>{t("options.folders")}</legend>
            <p id={noteId} className="options-form__note">
              {t("options.folders.note")}
            </p>
            {FILE_CATEGORIES.map((c) => (
              <div className="options-field" key={c}>
                <label htmlFor={`${fieldId}-${c}`}>{t(`categories.${c}`)}</label>
                <div className="options-folder">
                  <input
                    id={`${fieldId}-${c}`}
                    type="text"
                    dir="ltr"
                    spellCheck={false}
                    autoComplete="off"
                    value={folders[c]}
                    onChange={(e) => {
                      setSaved(null);
                      setFolders({ ...folders, [c]: e.target.value });
                    }}
                  />
                  <Button
                    aria-label={format(t("options.browseFor"), {
                      category: t(`categories.${c}`),
                    })}
                    onClick={() => void browse(c)}
                    {...(shell.available
                      ? {}
                      : { unavailableReason: t("options.browseDesktopOnly") })}
                  >
                    {t("options.browse")}
                  </Button>
                </div>
              </div>
            ))}
            {browseFailed && (
              <p role="alert" className="options-form__note">
                {t("options.browseFailed")}
              </p>
            )}
          </fieldset>
          <div className="options-field">
            <label htmlFor={collisionId}>{t("options.collision")}</label>
            <select
              id={collisionId}
              value={policy}
              onChange={(e) => {
                setSaved(null);
                setPolicy(e.target.value as CollisionPolicy);
              }}
            >
              {POLICIES.map((p) => (
                <option key={p} value={p}>
                  {t(`options.collision.${p}`)}
                </option>
              ))}
            </select>
          </div>
          <p className="options-form__note">{t("options.collision.note")}</p>
          <div>
            <Button
              onClick={() => void save()}
              {...(saving ? { unavailableReason: t("options.service.saving") } : {})}
            >
              {saving ? t("options.service.saving") : t("options.service.save")}
            </Button>
          </div>
          {saved?.kind === "saved" && (
            <p role="status" className="options-form__note">
              {t("options.service.saved")}
            </p>
          )}
          {saved?.kind === "failed" && (
            <div role="alert" className="options-form__note">
              <p>{t("options.service.saveFailed")}</p>
              {saved.failure.outcome === "rejected" && (
                <p>{t(`error.kind.${saved.failure.error.kind}`)}</p>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
