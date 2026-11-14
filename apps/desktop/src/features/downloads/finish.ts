// F08: what the finish action (sleep or shut down) still waits for. Decided from the service's
// snapshots only.
import type { DownloadSnapshot } from "@dm/contracts";

/** Seconds the countdown gives to cancel before the computer powers down. */
export const COUNTDOWN_SECONDS = 60;

const ENDED = new Set(["completed", "failed", "cancelled"]);

/**
 * Downloads the action waits for: every one the user wants running that has not ended, which
 * includes queued, transferring, verifying, scanning and finalizing (so the action never fires
 * while a file is still being checked or placed). A download the user paused does not hold it.
 */
export function pendingWork(items: readonly DownloadSnapshot[]): number {
  return items.filter((d) => d.desired_state === "running" && !ENDED.has(d.phase)).length;
}
