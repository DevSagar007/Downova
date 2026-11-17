import { useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useShell } from "../../services/ServiceProvider";
import type { AddToBrowserInfo, GuidedBrowser } from "../../services/service-client";
import { Button } from "../../ui/Button";

const ADDRESS: Record<GuidedBrowser, string> = {
  chrome: "chrome://extensions",
  edge: "edge://extensions",
};

/**
 * "Add Downova to Chrome/Edge" (ADR 0041). Without a store listing these browsers take the
 * extension only through their own extensions page ("Load unpacked"), and they ignore that page
 * when it is given on the command line. So the guide opens the browser, copies the page address
 * for the user to paste, and shows the extension folder installed beside Downova.
 */
export function AddToBrowser({
  browser,
  name,
  info,
}: {
  browser: GuidedBrowser;
  name: string;
  info: AddToBrowserInfo;
}) {
  const { t } = usePreferences();
  const shell = useShell();
  const [status, setStatus] = useState<string | null>(null);
  const address = ADDRESS[browser];
  const found = info.browsers_found.includes(browser);
  const run = (action: () => Promise<unknown>, done: string) => {
    setStatus(null);
    action().then(
      () => setStatus(done),
      () => setStatus(t("browsers.add.failed")),
    );
  };

  return (
    <section
      className="add-browser"
      aria-label={format(t("browsers.add.title"), { browser: name })}
    >
      <h4 className="add-browser__title">{format(t("browsers.add.title"), { browser: name })}</h4>
      {!info.extension_bundled ? (
        <p className="browsers__note">{t("browsers.add.notBundled")}</p>
      ) : (
        <>
          <p>{format(t("browsers.add.intro"), { browser: name })}</p>
          <ol className="add-browser__steps">
            <li>{format(t("browsers.add.step1"), { browser: name })}</li>
            <li>
              {t("browsers.add.step2")
                .split("{address}")
                .flatMap((part, i) =>
                  i === 0
                    ? [part]
                    : [
                        <bdi key="address" dir="ltr" className="add-browser__address">
                          {address}
                        </bdi>,
                        part,
                      ],
                )}
            </li>
            <li>{t("browsers.add.step3")}</li>
            <li>{t("browsers.add.step4")}</li>
            <li>{format(t("browsers.add.step5"), { browser: name })}</li>
          </ol>
          {!found && (
            <p className="browsers__note">
              {format(t("browsers.add.notFound"), { browser: name })}
            </p>
          )}
          <div className="browsers__actions">
            {found && (
              <Button onClick={() => run(() => shell.openBrowser(browser), "")}>
                {format(t("browsers.add.open"), { browser: name })}
              </Button>
            )}
            <Button
              onClick={() =>
                run(
                  () => shell.copyExtensionsAddress(browser),
                  format(t("browsers.add.copied"), { address }),
                )
              }
            >
              {format(t("browsers.add.copy"), { address })}
            </Button>
            <Button
              onClick={() => run(() => shell.showExtensionFolder(), t("browsers.add.folderShown"))}
            >
              {t("browsers.add.folder")}
            </Button>
          </div>
          {status && <p role="status">{status}</p>}
        </>
      )}
    </section>
  );
}
