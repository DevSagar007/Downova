// Validity of an active-transfer measurement (B2.33, REL-04).
//
// A resource sample means "Downova while N downloads transfer" only when the workload really
// was that. These rules turn a workload that was not into an explicit `invalid` result with
// reasons; the raw observations stay in the run's JSON, and every aggregate (summary.json,
// compare.mjs) counts only `ok` runs and lists the excluded ones with their reasons.
//
// Activity evidence is two observations of each job (at the start and at the end of the sample
// window): "gained bytes between the two" and "not completed at the end". That shows each
// download progressed during the window and was still unfinished at its end; it does not prove
// it transferred at every instant in between.

/** Reason codes, stable for reports and compare.mjs. */
export const REASONS = {
  JOB_IDS: "job_ids_not_distinct_or_missing",
  LIMITS: "limits_not_applied",
  NO_BYTES: "download_gained_no_bytes_in_window",
  EARLY: "download_completed_before_window_end",
  NOT_ACTIVE_AT_START: "download_not_transferring_at_window_start",
  TIMEOUT: "completion_timed_out",
  FAILED: "download_failed_or_cancelled",
  SHA: "sha256_mismatch",
};

/**
 * @param {object} w
 * @param {number} w.n                      files the scenario adds
 * @param {Array<string|null|undefined>} w.ids  job ids returned by `dm add`
 * @param {{global:number|null, main:number|null}} w.limits  read back after setting
 * @param {Record<string,{bytes:number, phase:string|null}>} w.atStart  per job at window start
 * @param {Record<string,{bytes:number, phase:string|null}>} w.atEnd    per job at window end
 * @param {{timed_out:boolean, files:Array<{job_id:string|null, phase:string|null, sha256_matches:boolean}>}} [w.completion]
 * @returns {{ valid: boolean, reasons: string[], per_job: object[] }}
 */
export function judgeActive({ n, ids, limits, atStart, atEnd, completion }) {
  const reasons = new Set();
  const distinct = new Set(ids.filter((id) => typeof id === "string" && id.length > 0));
  if (ids.length !== n || distinct.size !== n) reasons.add(REASONS.JOB_IDS);
  // Every file must be admitted at once: both limits (the smaller one applies) at least n.
  // Defaults are 3 and 3, so scenarios up to 3 need no change; above that both were set.
  if (!(Number(limits?.global) >= n && Number(limits?.main) >= n)) reasons.add(REASONS.LIMITS);
  const perJob = ids.map((id) => {
    const s = atStart?.[id];
    const e = atEnd?.[id];
    const row = {
      job_id: id ?? null,
      phase_at_start: s?.phase ?? null,
      phase_at_end: e?.phase ?? null,
      bytes_in_window: e && s ? e.bytes - s.bytes : null,
    };
    if (!(e && s && e.bytes > s.bytes)) reasons.add(REASONS.NO_BYTES);
    if (e?.phase === "completed") reasons.add(REASONS.EARLY);
    if (s?.phase !== "transferring") reasons.add(REASONS.NOT_ACTIVE_AT_START);
    return row;
  });
  if (completion) {
    if (completion.timed_out) reasons.add(REASONS.TIMEOUT);
    if (completion.files.some((f) => f.phase === "failed" || f.phase === "cancelled"))
      reasons.add(REASONS.FAILED);
    if (completion.files.some((f) => f.phase === "completed" && !f.sha256_matches))
      reasons.add(REASONS.SHA);
    if (!completion.timed_out && completion.files.some((f) => f.phase !== "completed"))
      reasons.add(REASONS.FAILED);
  }
  return { valid: reasons.size === 0, reasons: [...reasons], per_job: perJob };
}

/**
 * The run-level status from what happened: `error` (the run itself broke), `invalid` (the
 * workload was not what the scenario claims), otherwise `ok`.
 */
export function runStatus({ error, invalidReasons }) {
  if (error) return "error";
  if (invalidReasons?.length) return "invalid";
  return "ok";
}

/** Runs that count for medians, and the excluded ones with their reasons (never dropped). */
export function partition(results) {
  const ok = results.filter((r) => r.status === "ok");
  const excluded = results
    .filter((r) => r.status !== "ok")
    .map((r) => ({
      scenario: r.scenario,
      run: r.run ?? null,
      status: r.status,
      reasons: r.invalid_reasons ?? (r.error ? [r.error] : r.reason ? [r.reason] : []),
    }));
  return { ok, excluded };
}

/** Exit code for the command: non-zero when any requested run is not `ok` (results still written). */
export function exitCodeFor(results) {
  return results.length > 0 && results.every((r) => r.status === "ok") ? 0 : 1;
}
