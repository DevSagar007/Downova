import { useRef, useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";

/** The key a host-key error names (`server key <algorithm> SHA256:<base64>`, public data). */
export function presentedKey(detail: string | null | undefined) {
  const m = /server key ([a-z0-9@.-]{1,64}) (SHA256:[A-Za-z0-9+/]{43})/.exec(detail ?? "");
  return m?.[1] && m[2] ? { algorithm: m[1], fingerprint: m[2] } : null;
}

/** The host (lower case) and port of an `sftp://` link (default 22). */
export function sftpServer(link: string) {
  const m = /^sftp:\/\/(\[[^\]]+\]|[^/:?#]+)(?::(\d{1,5}))?/i.exec(link);
  if (!m?.[1]) return null;
  return { host: m[1].toLowerCase(), port: m[2] ? Number(m[2]) : 22 };
}

type Outcome = "trusted" | "failed" | null;

/**
 * ADR 0037: an SFTP download stopped at an untrusted or changed host key. An unknown key is shown
 * for the user to compare with one from the server's administrator, then trusted (the service
 * accepts only the key it observed) and the download retried. A changed key is never trusted from
 * here: someone may be intercepting the connection.
 */
export function HostKeyNote({
  job,
  onRetry,
}: {
  job: DownloadSnapshot;
  onRetry?: (() => void) | undefined;
}) {
  const { t } = usePreferences();
  const { trustHostKey } = useServiceActions();
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const inFlight = useRef(false);
  const kind = job.error?.kind;
  if (kind !== "HOST_KEY_UNKNOWN" && kind !== "HOST_KEY_CHANGED") return null;
  const key = presentedKey(job.error?.detail);
  const server = sftpServer(job.source_display);
  if (!key || !server) return null;
  const where = `${server.host}:${server.port}`;

  async function trust() {
    if (inFlight.current || !key || !server) return;
    inFlight.current = true;
    setBusy(true);
    try {
      await trustHostKey({ host: server.host, port: server.port, fingerprint: key.fingerprint });
      setOutcome("trusted");
      onRetry?.();
    } catch {
      setOutcome("failed");
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  if (kind === "HOST_KEY_CHANGED") {
    return (
      <Banner tone="danger" title={format(t("hostKey.changed.title"), { server: where })}>
        <p>{t("hostKey.changed.body")}</p>
        <p>
          {t("hostKey.presented")}{" "}
          <code dir="ltr" className="download-details__hash">
            {key.algorithm} {key.fingerprint}
          </code>
        </p>
        <p>{t("hostKey.changed.how")}</p>
      </Banner>
    );
  }
  return (
    <Banner tone="warning" title={format(t("hostKey.unknown.title"), { server: where })}>
      <p>{t("hostKey.unknown.body")}</p>
      <p>
        {t("hostKey.presented")}{" "}
        <code dir="ltr" className="download-details__hash">
          {key.algorithm} {key.fingerprint}
        </code>
      </p>
      {outcome && (
        <p role="status">{t(outcome === "trusted" ? "hostKey.trusted" : "hostKey.trustFailed")}</p>
      )}
      {outcome !== "trusted" && (
        <div className="download-details__actions">
          <Button
            variant="primary"
            onClick={() => void trust()}
            {...(busy ? { unavailableReason: t("hostKey.trusting") } : {})}
          >
            {t(busy ? "hostKey.trusting" : onRetry ? "hostKey.trustRetry" : "hostKey.trust")}
          </Button>
        </div>
      )}
    </Banner>
  );
}
