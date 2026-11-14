import { useId } from "react";
import { usePreferences } from "../../app/preferences";
import { Button } from "../../ui/Button";

/**
 * Options → Clipboard (F02). Off by default. While on and Downova runs, the desktop reads the
 * clipboard every few seconds and offers only links that name a file (http/https); the text is
 * never stored or sent anywhere. Ignored sites are listed here and can be removed.
 */
export function ClipboardSettings() {
  const { t, clipboardMonitor, setClipboardMonitor, ignoredHosts, unignoreHost } = usePreferences();
  const headingId = useId();
  const checkboxId = useId();
  const noteId = useId();
  return (
    <section className="options-section" aria-labelledby={headingId}>
      <h3 id={headingId} className="options-section__heading">
        {t("clipboard.heading")}
      </h3>
      <div className="options-check">
        <input
          id={checkboxId}
          type="checkbox"
          checked={clipboardMonitor}
          aria-describedby={noteId}
          onChange={(e) => setClipboardMonitor(e.target.checked)}
        />
        <label htmlFor={checkboxId}>{t("clipboard.enable")}</label>
      </div>
      <p id={noteId} className="options-form__note">
        {t("clipboard.note")}
      </p>
      {ignoredHosts.length > 0 && (
        <>
          <p className="options-form__note">{t("clipboard.ignored")}</p>
          <ul className="options-list">
            {ignoredHosts.map((host) => (
              <li key={host}>
                <bdi dir="ltr">{host}</bdi>{" "}
                <Button onClick={() => unignoreHost(host)}>{t("clipboard.unignore")}</Button>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
