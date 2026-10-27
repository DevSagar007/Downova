// Presentation helpers. They only format service state; they never estimate or invent progress.
import type { ByteCount, DownloadSnapshot } from "@dm/contracts";
import { format, type MessageKey } from "../../i18n/messages";

type Translate = (key: MessageKey) => string;

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

/** Formats a decimal-string byte count (binary multiples, labelled KB/MB as users expect). */
export function formatBytes(value: ByteCount): string {
  let bytes = BigInt(value);
  let unit = 0;
  let remainder = 0n;
  while (bytes >= 1024n && unit < UNITS.length - 1) {
    remainder = bytes % 1024n;
    bytes /= 1024n;
    unit += 1;
  }
  if (unit === 0) return `${bytes} B`;
  const tenths = Number((remainder * 10n) / 1024n);
  return bytes >= 100n ? `${bytes} ${UNITS[unit]}` : `${bytes}.${tenths} ${UNITS[unit]}`;
}

/** Percentage with one decimal, or null when the total is unknown or zero. */
export function percentOf(done: ByteCount, total: ByteCount | null): string | null {
  if (total === null || BigInt(total) === 0n) return null;
  const permille = (BigInt(done) * 1000n) / BigInt(total);
  return `${permille / 10n}.${permille % 10n}%`;
}

export function statusText(download: DownloadSnapshot, t: Translate): string {
  if (download.phase === "completed") return t("status.completed");
  if (download.phase === "failed") return t("status.failed");
  if (download.phase === "cancelled") return t("status.cancelled");
  if (download.desired_state === "paused") return t("status.paused");
  const blocker = download.blockers[0];
  if (blocker) {
    return format(t("status.waiting"), { reason: t(`blocker.${blocker}`) });
  }
  if (download.phase === "transferring") {
    // Without a known total there is no percentage; the received byte count is still true.
    const percent =
      percentOf(download.downloaded_bytes, download.total_bytes) ??
      (BigInt(download.downloaded_bytes) > 0n ? formatBytes(download.downloaded_bytes) : "");
    return format(t("status.transferring"), { percent }).trim();
  }
  return t(`status.${download.phase}`);
}

/** The Description cell: the error summary, or how a completed file was verified. */
export function descriptionText(download: DownloadSnapshot, t: Translate): string {
  const error = errorText(download, t);
  if (error) return error.summary;
  if (download.phase === "completed" && download.verification) {
    return t(`verification.${download.verification.source}`);
  }
  return "";
}

export function sizeText(download: DownloadSnapshot, t: Translate): string {
  return download.total_bytes === null ? t("table.unknownSize") : formatBytes(download.total_bytes);
}

export interface ErrorText {
  /** What went wrong, from the stable `kind`. */
  summary: string;
  /** What the user can do, from `next_action`; null when there is nothing useful to offer. */
  action: string | null;
  /** Whether the download continues, retries or has stopped; null when nothing applies. */
  followUp: string | null;
}

/**
 * Localized explanation of a download's error (CORE-11). Built only from the contract's `kind`,
 * `next_action`, `phase`, `desired_state` and `blockers`: the service's English `detail` is never
 * parsed or shown here.
 */
export function errorText(download: DownloadSnapshot, t: Translate): ErrorText | null {
  const error = download.error;
  if (!error) return null;
  const summary = t(`error.kind.${error.kind}`);
  const action = error.next_action === "none" ? null : t(`error.action.${error.next_action}`);
  if (download.phase === "failed") {
    // Both actions already say how to get the file; a failed download never resumes.
    const saysHow = error.next_action === "refresh_link" || error.next_action === "sign_in";
    return { summary, action, followUp: saysHow ? null : t("error.followUp.failed") };
  }
  if (download.desired_state === "paused") return { summary, action, followUp: null };
  const waitsForDisk = download.blockers.some(
    (b) => b === "disk_unavailable" || b === "volume_missing",
  );
  if (waitsForDisk) return { summary, action, followUp: t("error.followUp.continues") };
  // Alive with an error and no disk blocker: an automatic retry is scheduled (COR-04).
  return { summary, action: null, followUp: t("error.followUp.autoRetry") };
}

/** Visual tone of a status (paired with an icon and text; never colour alone). */
export type StatusTone = "neutral" | "info" | "warning" | "success" | "danger";

/** The status's kind, for its icon and tone. Manual pause outranks policy blockers. */
export type StatusKind =
  | "completed"
  | "failed"
  | "cancelled"
  | "paused"
  | "waiting"
  | "queued"
  | "connecting"
  | "transferring"
  | "verifying"
  | "scanning"
  | "finalizing";

export function statusKind(download: DownloadSnapshot): StatusKind {
  switch (download.phase) {
    case "completed":
    case "failed":
    case "cancelled":
      return download.phase;
    default:
      break;
  }
  if (download.desired_state === "paused") return "paused";
  if (download.blockers.length > 0) return "waiting";
  switch (download.phase) {
    case "probing":
      return "connecting";
    case "queued":
    case "transferring":
    case "verifying":
    case "scanning":
    case "finalizing":
      return download.phase;
  }
}

export const STATUS_TONE: Record<StatusKind, StatusTone> = {
  completed: "success",
  failed: "danger",
  cancelled: "neutral",
  paused: "neutral",
  waiting: "warning",
  queued: "neutral",
  connecting: "info",
  transferring: "info",
  verifying: "info",
  scanning: "info",
  finalizing: "info",
};

/**
 * Progress in thousandths (0–1000) from the service's decimal-string byte counts, or null when
 * there is nothing honest to draw: a finished job, an unknown or zero total. Exact for any size
 * (BigInt); a count above the total is shown full, never more.
 */
export function progressPermille(download: DownloadSnapshot): number | null {
  if (["completed", "failed", "cancelled"].includes(download.phase)) return null;
  if (download.total_bytes === null) return null;
  const total = BigInt(download.total_bytes);
  if (total === 0n) return null;
  const done = BigInt(download.downloaded_bytes);
  const permille = (done * 1000n) / total;
  return Number(permille > 1000n ? 1000n : permille);
}
