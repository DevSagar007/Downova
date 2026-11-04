// node --test scripts/perf/validity.test.mjs (part of `pnpm verify`, group test).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { median, p95, runLevel } from "./stats.mjs";
import { REASONS, exitCodeFor, judgeActive, partition, runStatus } from "./validity.mjs";

const ids = ["job_a", "job_b", "job_c"];
const obs = (bytes, phase = "transferring") =>
  Object.fromEntries(ids.map((id, i) => [id, { bytes: bytes + i, phase }]));
const done = (over = {}) => ({
  timed_out: false,
  files: ids.map((id) => ({ job_id: id, phase: "completed", sha256_matches: true, ...over })),
});
const good = () => ({
  n: 3,
  ids: [...ids],
  limits: { global: 3, main: 3 },
  atStart: obs(100),
  atEnd: obs(5000),
  completion: done(),
});

test("a complete, active, intact workload is valid", () => {
  const j = judgeActive(good());
  assert.deepEqual(j, {
    valid: true,
    reasons: [],
    per_job: ids.map((id, i) => ({
      job_id: id,
      phase_at_start: "transferring",
      phase_at_end: "transferring",
      bytes_in_window: 4900,
    })),
  });
  assert.equal(runStatus({ invalidReasons: j.reasons }), "ok");
});

test("each broken condition makes the run invalid with its reason", () => {
  const cases = [
    [(w) => (w.atEnd[ids[1]].bytes = w.atStart[ids[1]].bytes), REASONS.NO_BYTES],
    [(w) => (w.atEnd[ids[0]].phase = "completed"), REASONS.EARLY],
    [(w) => (w.atStart[ids[2]].phase = "queued"), REASONS.NOT_ACTIVE_AT_START],
    [(w) => (w.limits = { global: 2, main: 3 }), REASONS.LIMITS],
    [(w) => (w.limits = { global: null, main: 6 }), REASONS.LIMITS],
    [(w) => (w.ids[2] = undefined), REASONS.JOB_IDS],
    [(w) => (w.ids[2] = "job_a"), REASONS.JOB_IDS],
    [(w) => (w.completion = { ...done(), timed_out: true }), REASONS.TIMEOUT],
    [(w) => (w.completion = done({ phase: "failed" })), REASONS.FAILED],
    [(w) => (w.completion = done({ sha256_matches: false })), REASONS.SHA],
  ];
  for (const [breakIt, reason] of cases) {
    const w = good();
    breakIt(w);
    const j = judgeActive(w);
    assert.equal(j.valid, false, `expected ${reason}`);
    assert.ok(j.reasons.includes(reason), `${reason} in ${j.reasons}`);
    assert.equal(runStatus({ invalidReasons: j.reasons }), "invalid");
  }
});

test("six files need both limits at six or more", () => {
  const six = ["1", "2", "3", "4", "5", "6"].map((k) => `job_${k}`);
  const w = {
    n: 6,
    ids: six,
    limits: { global: 6, main: 3 },
    atStart: Object.fromEntries(six.map((id) => [id, { bytes: 1, phase: "transferring" }])),
    atEnd: Object.fromEntries(six.map((id) => [id, { bytes: 9, phase: "transferring" }])),
  };
  assert.deepEqual(judgeActive(w).reasons, [REASONS.LIMITS]);
  w.limits.main = 6;
  assert.equal(judgeActive(w).valid, true);
});

test("an error outranks invalid; the exit code fails on any run that is not ok", () => {
  assert.equal(runStatus({ error: "boom", invalidReasons: ["x"] }), "error");
  assert.equal(exitCodeFor([{ status: "ok" }, { status: "ok" }]), 0);
  assert.equal(exitCodeFor([{ status: "ok" }, { status: "invalid" }]), 1);
  assert.equal(exitCodeFor([{ status: "not run" }]), 1);
  assert.equal(exitCodeFor([]), 1);
});

test("partition keeps excluded runs with their reasons", () => {
  const { ok, excluded } = partition([
    { scenario: "active_6", run: 1, status: "ok" },
    { scenario: "active_6", run: 2, status: "invalid", invalid_reasons: [REASONS.EARLY] },
    { scenario: "active_6", run: 3, status: "error", error: "fixture did not announce itself" },
    { scenario: "idle_visible", run: 1, status: "not run", reason: "another Downova desktop" },
  ]);
  assert.equal(ok.length, 1);
  assert.deepEqual(
    excluded.map((e) => [e.run, e.status, e.reasons]),
    [
      [2, "invalid", [REASONS.EARLY]],
      [3, "error", ["fixture did not announce itself"]],
      [1, "not run", ["another Downova desktop"]],
    ],
  );
});

test("p95 needs 20 values; run-level latency never pools observations", () => {
  assert.equal(p95(Array.from({ length: 19 }, (_, i) => i)), null);
  assert.equal(p95(Array.from({ length: 20 }, (_, i) => i + 1)), 19);
  assert.equal(median([3, null, 1, 2]), 2);
  // 5 runs × 20 observations = 100 observations, but only 5 runs: no p95.
  const perRun = Array.from({ length: 5 }, (_, r) => Array.from({ length: 20 }, () => 10 + r));
  assert.deepEqual(runLevel(perRun), {
    runs: 5,
    observations: 100,
    median_of_run_medians: 12,
    run_level_p95: null,
  });
});

test("compare.mjs aggregates only ok runs and lists the excluded ones", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-compare-"));
  const sample = (mib) => ({
    private_bytes_total: mib * 1048576,
    cpu_percent_of_machine: 1,
    process_count: 8,
    by_role: {
      desktop: { private_bytes: 1048576 },
      webview: { private_bytes: 1048576 },
      service: { private_bytes: 1048576 },
    },
    processes: [{ role: "webview", private_bytes: 1048576 }],
  });
  const write = (run, status, mib, extra = {}) =>
    fs.writeFileSync(
      path.join(dir, `run${run}-active_6.json`),
      JSON.stringify({ scenario: "active_6", run, status, sample: sample(mib), ...extra }),
    );
  write(1, "ok", 200, {
    activity: { bytes_in_window: 1048576, limits: { global: 6, main: 6 } },
    completion: { completion_ms_from_add: 45000, all_sha256_match: true },
  });
  write(2, "ok", 220, {
    activity: { bytes_in_window: 1048576, limits: { global: 6, main: 6 } },
    completion: { completion_ms_from_add: 47000, all_sha256_match: true },
  });
  // An invalid run with a huge value: must not move the median.
  write(3, "invalid", 9999, { invalid_reasons: [REASONS.EARLY] });
  const r = spawnSync(process.execPath, [path.join(import.meta.dirname, "compare.mjs"), dir], {
    encoding: "utf8",
  });
  fs.rmSync(dir, { recursive: true, force: true });
  assert.equal(r.status, 0, r.stderr);
  const row = JSON.parse(r.stdout)[dir].active_6;
  assert.equal(row.n, 2);
  assert.equal(row.not_ok, 1);
  assert.deepEqual(row.excluded[0].reasons, [REASONS.EARLY]);
  assert.equal(row.private_mib.median, 210);
  assert.deepEqual(row.private_mib.range, [200, 220]);
  assert.equal(row.activity.n, 2);
  assert.equal(row.activity.completion_ms.median, 46000);
});
