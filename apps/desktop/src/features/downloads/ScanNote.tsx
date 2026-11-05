import type { DownloadSnapshot, ScanRecord } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import type { MessageKey } from "../../i18n/messages";
import { Banner } from "../../ui/Banner";

const TONE = {
  scanning: "info",
  attachment_checked: "info",
  blocked: "danger",
  unavailable: "warning",
  skipped: "warning",
} as const;

/**
 * The attachment check of a finished download, in plain words (F07, E06, ADR 0026). Built only
 * from the service's durable `scan` record. It never says "safe": a check that passed is described
 * as what Windows did, and Mark of the Web is described as origin only. A download finished before
 * this existed has no record and shows nothing.
 */
export function ScanNote({ job }: { job: DownloadSnapshot }) {
  const { t } = usePreferences();
  const scan: ScanRecord | undefined = job.scan;
  if (!scan) return null;
  const platformSkip = scan.status === "skipped" && scan.reason === "platform_not_supported";
  const title: MessageKey = platformSkip
    ? "scan.title.platform"
    : (`scan.title.${scan.status}` as MessageKey);
  const lines: string[] = [];
  if (scan.reason && !platformSkip) lines.push(t(`scan.reason.${scan.reason}` as MessageKey));
  lines.push(t(platformSkip ? "scan.body.platform" : (`scan.body.${scan.status}` as MessageKey)));
  if (scan.origin_marking && scan.origin_marking !== "not_supported_on_platform") {
    lines.push(t(`scan.marking.${scan.origin_marking}` as MessageKey));
  }
  return (
    <Banner tone={platformSkip ? "info" : TONE[scan.status]} title={t(title)}>
      {lines.map((l) => (
        <p key={l}>{l}</p>
      ))}
    </Banner>
  );
}
