import type { DownloadSnapshot, QueueInfo } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { MAIN_QUEUE, fileTypeOf, queueOf } from "./categories";
import { errorText, formatBytes, sizeText, statusText } from "./present";
import "./PropertiesDialog.css";

/**
 * File → Properties (IDM parity): everything the service reports about the selected download,
 * read-only. Only the service's snapshot is shown; nothing it does not report (such as a folder
 * or a date) is guessed. The address is the service's display form (protected parts redacted).
 */
export function PropertiesDialog({
  download,
  queues,
  onClose,
}: {
  download: DownloadSnapshot;
  queues: QueueInfo[];
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const none = t("properties.none");
  const queueId = queueOf(download);
  const queueName =
    queueId === MAIN_QUEUE
      ? t("categories.mainQueue")
      : (queues.find((q) => q.queue_id === queueId)?.name ?? queueId);
  const fileType = fileTypeOf(download.display_name);
  const category = download.category
    ? t(`categories.${download.category}`)
    : fileType
      ? format(t("properties.categoryAuto"), { category: t(`categories.${fileType}`) })
      : none;
  const error = errorText(download, t);
  const rows: [string, string][] = [
    [t("properties.name"), download.display_name],
    [t("properties.address"), download.source_display],
    [t("properties.status"), statusText(download, t)],
    [t("properties.size"), sizeText(download, t)],
    [t("properties.downloaded"), formatBytes(download.downloaded_bytes)],
    [t("properties.category"), category],
    [t("properties.queue"), queueName],
    [t("properties.priority"), t(`priority.${download.priority ?? "normal"}`)],
    [
      t("properties.speedLimit"),
      download.speed_limit_bytes_per_second
        ? format(t("properties.perSecond"), {
            size: formatBytes(download.speed_limit_bytes_per_second),
          })
        : t("properties.overallOnly"),
    ],
    [
      t("properties.checksum"),
      download.verification
        ? `${t(`verification.${download.verification.source}`)}: ${download.verification.sha256}`
        : download.checksum_expected
          ? t("properties.checksumPending")
          : none,
    ],
  ];
  if (download.auth) {
    rows.push([
      t("properties.signIn"),
      `${download.auth.origin}. ${t(`auth.mode.${download.auth.mode}`)}`,
    ]);
  }
  if (error) {
    rows.push([
      t("properties.error"),
      [error.summary, error.action, error.followUp].filter(Boolean).join(" "),
    ]);
  }
  return (
    <Dialog
      title={t("properties.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={<Button onClick={onClose}>{t("batch.close")}</Button>}
    >
      <dl className="properties">
        {rows.map(([label, value]) => (
          <div key={label} className="properties__row">
            <dt>{label}</dt>
            <dd>
              <bdi>{value}</bdi>
            </dd>
          </div>
        ))}
      </dl>
    </Dialog>
  );
}
