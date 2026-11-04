#!/usr/bin/env node
// Aggregates alternating measurement runs of two packages (REL-04, B2.24).
//
//   node scripts/perf/compare.mjs perf-results/<a> perf-results/<b> [--out perf-results/<x>.json]
//
// Each argument is a folder holding one or more `measure.mjs` output folders (one per alternating
// run, e.g. perf-results/cmp-b223/r1 … r5). Every raw `run*-<scenario>.json` below it counts as one
// observation. Prints medians (and p95 only with ≥ 20 observations) per scenario and package, with
// the sample count, so a reader can see how much each number rests on.
import fs from "node:fs";
import path from "node:path";

const argv = process.argv.slice(2);
const outIdx = argv.indexOf("--out");
const out = outIdx >= 0 ? argv[outIdx + 1] : null;
const dirs = argv.filter((a, i) => !a.startsWith("--") && (outIdx < 0 || i !== outIdx + 1));

const MiB = (b) => b / 1048576;
const median = (xs) => {
  const s = xs.filter((v) => v != null).sort((a, b) => a - b);
  if (!s.length) return null;
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};
const p95 = (xs) => {
  const s = xs.filter((v) => v != null).sort((a, b) => a - b);
  return s.length >= 20 ? s[Math.ceil(s.length * 0.95) - 1] : null;
};
const range = (xs) => {
  const s = xs.filter((v) => v != null);
  return s.length ? [Math.min(...s), Math.max(...s)] : null;
};

function runs(dir) {
  const found = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (/^run\d+-.+\.json$/.test(e.name)) found.push(JSON.parse(fs.readFileSync(p, "utf8")));
    }
  };
  walk(dir);
  return found;
}

const result = {};
for (const dir of dirs) {
  const all = runs(dir);
  const byScenario = {};
  for (const r of all) (byScenario[r.scenario] ??= []).push(r);
  result[dir] = {};
  for (const [s, rs] of Object.entries(byScenario)) {
    const ok = rs.filter((r) => r.status === "ok");
    const row = { n: ok.length, not_ok: rs.length - ok.length };
    if (s === "service_start") {
      row.first_start_ms = {
        median: median(ok.map((r) => r.first_start_ms)),
        range: range(ok.map((r) => r.first_start_ms)),
      };
      row.second_start_ms = {
        median: median(ok.map((r) => r.second_start_ms)),
        range: range(ok.map((r) => r.second_start_ms)),
      };
      row.service_private_mib = median(
        ok.map((r) => r.second_private_bytes && MiB(r.second_private_bytes)),
      );
    } else {
      const withSample = ok.filter((r) => r.sample);
      const largest = withSample.map((r) =>
        Math.max(
          0,
          ...r.sample.processes
            .filter((p) => p.role === "webview")
            .map((p) => MiB(p.private_bytes)),
        ),
      );
      Object.assign(row, {
        private_mib: {
          median: median(withSample.map((r) => MiB(r.sample.private_bytes_total))),
          range: range(withSample.map((r) => MiB(r.sample.private_bytes_total))),
        },
        by_role_mib: Object.fromEntries(
          ["desktop", "webview", "service"].map((k) => [
            k,
            median(withSample.map((r) => MiB(r.sample.by_role[k].private_bytes))),
          ]),
        ),
        largest_webview_mib: largest.map((v) => Math.round(v)),
        cpu_percent: median(withSample.map((r) => r.sample.cpu_percent_of_machine)),
        processes: median(withSample.map((r) => r.sample.process_count)),
        service_ready_ms: {
          median: median(ok.map((r) => r.service_ready_ms)),
          range: range(ok.map((r) => r.service_ready_ms)),
        },
        ui_dom_ready_ms: median(ok.map((r) => r.ui_dom_ready_ms)),
        ui_first_paint_ms: {
          n: ok.filter((r) => r.ui_first_paint_ms != null).length,
          median: median(ok.map((r) => r.ui_first_paint_ms)),
        },
        list_ready_ms: {
          median: median(ok.map((r) => r.list_ready_ms)),
          upper_bounds: ok.filter((r) => r.list_ready_bound === "upper").length,
        },
      });
      const searches = ok.flatMap((r) => r.ui?.search_ms ?? []);
      const scrolls = ok.flatMap((r) => r.ui?.scroll_ms ?? []);
      if (searches.length)
        row.ui = {
          dom_nodes: median(ok.map((r) => r.ui?.dom_nodes)),
          first_rows_ms: median(ok.map((r) => r.list_first_rows_ms)),
          search_ms: { n: searches.length, median: median(searches), p95: p95(searches) },
          scroll_ms: { n: scrolls.length, median: median(scrolls), p95: p95(scrolls) },
        };
      // Active scenarios (B2.32): proof of activity over the window, completion and integrity.
      const withActivity = ok.filter((r) => r.activity);
      if (withActivity.length)
        row.activity = {
          n: withActivity.length,
          runs_every_download_active: withActivity.filter(
            (r) =>
              r.activity.every_download_gained_bytes && r.activity.finished_before_window_end === 0,
          ).length,
          completion_ms: {
            median: median(withActivity.map((r) => r.completion?.completion_ms_from_add)),
            range: range(withActivity.map((r) => r.completion?.completion_ms_from_add)),
          },
          bytes_in_window_mib: median(withActivity.map((r) => MiB(r.activity.bytes_in_window))),
          runs_all_sha256_match: withActivity.filter((r) => r.completion?.all_sha256_match).length,
          limits: [...new Set(withActivity.map((r) => JSON.stringify(r.activity.limits)))],
        };
      const cycles = ok.filter((r) => r.cycles).map((r) => r.cycles);
      if (cycles.length)
        row.cycles = {
          js_heap_after_gc_mib: cycles.map((c) =>
            c.js_heap_after_gc.map((v) => +MiB(v).toFixed(2)),
          ),
          listeners: cycles.map((c) => c.listeners),
          nodes: cycles.map((c) => c.nodes),
          private_mib_before_after: cycles.map((c) =>
            [c.private_bytes_first, c.private_bytes_after].map((v) =>
              v == null ? null : +MiB(v).toFixed(1),
            ),
          ),
          reattach_ms: cycles.map((c) =>
            c.steps
              .filter((s) => "window_reattached_ms" in s)
              .map((s) => s.window_reattached_ms && Math.round(s.window_reattached_ms)),
          ),
        };
    }
    result[dir][s] = row;
  }
}
const text = JSON.stringify(result, null, 2);
if (out) fs.writeFileSync(out, text);
console.log(text);
