// Summary statistics shared by measure.mjs and compare.mjs (REL-04).

const clean = (xs) => xs.filter((v) => v != null && Number.isFinite(v)).sort((a, b) => a - b);

export function median(xs) {
  const s = clean(xs);
  if (!s.length) return null;
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
}

/** Nearest-rank p95, only with at least 20 values (research §11); otherwise null. */
export function p95(xs) {
  const s = clean(xs);
  return s.length >= 20 ? s[Math.ceil(s.length * 0.95) - 1] : null;
}

export function range(xs) {
  const s = clean(xs);
  return s.length ? [s[0], s[s.length - 1]] : null;
}

/**
 * Run-level latency summary: each run's median, then the median of those; p95 only over ≥ 20
 * independent runs. Observations within one run share a process and a warm cache, so pooling
 * them would overstate the sample size.
 * @param {number[][]} perRun observations per run
 */
export function runLevel(perRun) {
  const medians = perRun.filter((xs) => xs.length).map((xs) => median(xs));
  return {
    runs: medians.length,
    observations: perRun.reduce((s, xs) => s + xs.length, 0),
    median_of_run_medians: median(medians),
    run_level_p95: p95(medians),
  };
}
