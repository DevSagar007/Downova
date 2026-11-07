import { useRef, useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";

const TONE = {
  available: "info",
  missing: "warning",
  locked: "warning",
  unavailable: "warning",
  expired: "warning",
  forgotten: "info",
} as const;

type Outcome = "forgotten" | "failed" | null;

/**
 * The sign-in a download uses (BRW-05, SEC-02, ADR 0027): the site, the browser, whether it is
 * kept only in memory or remembered in the OS credential store, and its state, with Forget. It
 * is built from the snapshot's `auth` metadata only; the webview never holds a cookie or token.
 * Rendered with `key={job_id}`, so a late answer for one download never shows on another.
 */
export function SignInNote({ job }: { job: DownloadSnapshot }) {
  const { t } = usePreferences();
  const { forgetCredential } = useServiceActions();
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const auth = job.auth;
  if (!auth) return null;
  const canForget = auth.state !== "forgotten" && outcome !== "forgotten";
  const waiting = job.blockers.includes("sign_in_required");
  const needsReconnect =
    waiting ||
    (["missing", "expired", "forgotten"] as const).some((s) => s === auth.state) ||
    outcome === "forgotten";
  const forget = async () => {
    if (inFlight.current) return; // One request, however fast the clicks come.
    inFlight.current = true;
    setBusy(true);
    try {
      await forgetCredential(auth.credential_id);
      setOutcome("forgotten");
    } catch {
      setOutcome("failed");
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  return (
    <Banner tone={TONE[auth.state]} title={format(t("auth.title"), { origin: auth.origin })}>
      <p>
        {t(`auth.browser.${auth.browser}` as MessageKey)}.{" "}
        {t(`auth.mode.${auth.mode}` as MessageKey)}
      </p>
      <p data-auth-state={auth.state}>{t(`auth.state.${auth.state}` as MessageKey)}</p>
      {/* ADR 0028: a held download is reconnected from the browser (never from here: the
          desktop never receives a cookie). */}
      {waiting && <p>{t("auth.waiting")}</p>}
      {needsReconnect && <p>{format(t("auth.reconnectHow"), { origin: auth.origin })}</p>}
      {outcome && (
        <p role="status">{t(outcome === "forgotten" ? "auth.forgotten" : "auth.forgetFailed")}</p>
      )}
      {canForget && (
        <div className="download-details__actions">
          <Button
            onClick={() => void forget()}
            {...(busy ? { unavailableReason: t("auth.forgetting") } : {})}
            aria-busy={busy}
          >
            {t(busy ? "auth.forgetting" : "auth.forget")}
          </Button>
        </div>
      )}
    </Banner>
  );
}
