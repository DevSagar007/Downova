import { useEffect, useId, useState } from "react";
import { usePreferences, type ThemePreference } from "../../app/preferences";
import { LOCALES, type Locale } from "../../i18n/messages";
import { useShell } from "../../services/ServiceProvider";
import type { CloseBehaviour, DesktopSettings } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { DownloadSettings } from "./DownloadSettings";
import { HistorySettings } from "./HistorySettings";
import "./OptionsDialog.css";

const THEMES: ThemePreference[] = ["system", "light", "dark"];
const CLOSE_BEHAVIOURS: CloseBehaviour[] = ["hide_to_tray", "close_window"];

export function OptionsDialog({ onClose }: { onClose: () => void }) {
  const { t, locale, setLocale, theme, setTheme } = usePreferences();
  const languageId = useId();
  const themeId = useId();
  const closeId = useId();
  const closeNoteId = useId();
  const shell = useShell();
  // Saved by the desktop shell (not this window's storage): it decides before the UI is asked.
  const [desktop, setDesktop] = useState<DesktopSettings | null>(null);
  const [saveFailed, setSaveFailed] = useState(false);
  useEffect(() => {
    let active = true;
    void shell.getSettings().then((s) => {
      if (active) setDesktop(s);
    });
    return () => {
      active = false;
    };
  }, [shell]);

  async function changeClose(behaviour: CloseBehaviour) {
    setSaveFailed(false);
    try {
      setDesktop(await shell.setCloseBehaviour(behaviour));
    } catch {
      setSaveFailed(true);
    }
  }

  return (
    <Dialog
      title={t("options.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <Button variant="primary" onClick={onClose}>
          {t("options.done")}
        </Button>
      }
    >
      <form className="options-form" onSubmit={(e) => e.preventDefault()}>
        <div className="options-field">
          <label htmlFor={languageId}>{t("options.language")}</label>
          <select
            id={languageId}
            value={locale}
            onChange={(e) => setLocale(e.target.value as Locale)}
          >
            {LOCALES.map((l) => (
              <option key={l} value={l} lang={l}>
                {t(`language.${l}`)}
              </option>
            ))}
          </select>
        </div>
        <div className="options-field">
          <label htmlFor={themeId}>{t("options.theme")}</label>
          <select
            id={themeId}
            value={theme}
            onChange={(e) => setTheme(e.target.value as ThemePreference)}
          >
            {THEMES.map((value) => (
              <option key={value} value={value}>
                {t(`options.theme.${value}`)}
              </option>
            ))}
          </select>
        </div>
        <div className="options-field">
          <label htmlFor={closeId}>{t("options.closeBehaviour")}</label>
          {desktop ? (
            <select
              id={closeId}
              value={desktop.close_behaviour}
              aria-describedby={closeNoteId}
              onChange={(e) => void changeClose(e.target.value as CloseBehaviour)}
            >
              {CLOSE_BEHAVIOURS.map((value) => (
                <option key={value} value={value}>
                  {t(`options.close.${value}`)}
                </option>
              ))}
            </select>
          ) : (
            <p id={closeId} className="options-form__note">
              {t("options.close.unavailable")}
            </p>
          )}
        </div>
        {desktop && (
          <p id={closeNoteId} className="options-form__note">
            {desktop.tray_available ? t("options.close.note") : t("options.close.noTray")}
          </p>
        )}
        {saveFailed && (
          <p role="alert" className="options-form__note">
            {t("options.close.saveFailed")}
          </p>
        )}
        <p className="options-form__note">{t("options.localNote")}</p>
      </form>
      <DownloadSettings />
      <HistorySettings />
    </Dialog>
  );
}
