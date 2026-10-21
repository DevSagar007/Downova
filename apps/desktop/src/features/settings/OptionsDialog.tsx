import { useId } from "react";
import { usePreferences, type ThemePreference } from "../../app/preferences";
import { LOCALES, type Locale } from "../../i18n/messages";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import "./OptionsDialog.css";

const THEMES: ThemePreference[] = ["system", "light", "dark"];

export function OptionsDialog({ onClose }: { onClose: () => void }) {
  const { t, locale, setLocale, theme, setTheme } = usePreferences();
  const languageId = useId();
  const themeId = useId();

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
        <p className="options-form__note">{t("options.localNote")}</p>
      </form>
    </Dialog>
  );
}
