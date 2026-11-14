import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { hostOf } from "./intake";
import "./ClipboardPrompt.css";

/**
 * F02: copied links offered for download. A prompt only: "Download…" opens a dialog that is
 * already filled in, and the user still chooses Add there. Nothing starts from a copy.
 */
export function ClipboardPrompt({
  links,
  onDownload,
  onDismiss,
}: {
  links: string[];
  /** Opens Add URL (one link) or the batch dialog (several), prefilled. */
  onDownload: () => void;
  onDismiss: () => void;
}) {
  const { t, ignoreHost } = usePreferences();
  const first = links[0];
  if (!first) return null;
  const host = hostOf(first);
  return (
    <div className="clipboard-prompt">
      <Banner tone="info" title={t("clipboard.prompt.title")}>
        <p>
          {links.length === 1 ? (
            <bdi dir="ltr" className="clipboard-prompt__link">
              {first}
            </bdi>
          ) : (
            format(t("clipboard.prompt.many"), { count: links.length })
          )}
        </p>
        <div className="clipboard-prompt__actions">
          <Button variant="primary" onClick={onDownload}>
            {t("clipboard.prompt.download")}
          </Button>
          <Button onClick={onDismiss}>{t("clipboard.prompt.notNow")}</Button>
          {host && links.every((l) => hostOf(l) === host) && (
            <Button
              onClick={() => {
                ignoreHost(host);
                onDismiss();
              }}
            >
              {format(t("clipboard.prompt.ignoreSite"), { host })}
            </Button>
          )}
        </div>
      </Banner>
    </div>
  );
}
