import type { DownloadSnapshot, JobId } from "@dm/contracts";
import { toCallError, type ServiceCallError } from "../../services/service-client";

/** At most this many downloads.v1.remove requests are in flight at once. */
export const CLEAR_FINISHED_CONCURRENCY = 3;

const FINISHED = new Set(["completed", "failed", "cancelled"]);

/** Removal is only for finished jobs; unfinished, paused, blocked and finalizing stay. */
export function isFinished(download: DownloadSnapshot): boolean {
  return FINISHED.has(download.phase) && download.removed !== true;
}

export interface ClearTarget {
  jobId: JobId;
  name: string;
}

export interface ClearFailure {
  jobId: JobId;
  name: string;
  error: ServiceCallError;
}

export interface ClearOutcome {
  succeeded: JobId[];
  failed: ClearFailure[];
}

/**
 * Removes a frozen set of finished jobs with bounded concurrency. Each job is reported only after
 * the service acknowledges it; one failure never stops the others.
 */
export async function clearFinished(
  targets: readonly ClearTarget[],
  remove: (jobId: JobId) => Promise<DownloadSnapshot>,
  concurrency = CLEAR_FINISHED_CONCURRENCY,
): Promise<ClearOutcome> {
  const errors = new Map<number, ServiceCallError>();
  let cursor = 0;
  const limit = Math.max(1, Math.min(concurrency, targets.length));
  const worker = async () => {
    while (true) {
      const index = cursor++;
      const target = targets[index];
      if (!target) return;
      try {
        await remove(target.jobId);
      } catch (thrown) {
        errors.set(index, toCallError(thrown, true));
      }
    }
  };
  await Promise.all(Array.from({ length: limit }, worker));
  const succeeded: JobId[] = [];
  const failed: ClearFailure[] = [];
  targets.forEach((target, index) => {
    const error = errors.get(index);
    if (error) failed.push({ ...target, error });
    else succeeded.push(target.jobId);
  });
  return { succeeded, failed };
}
