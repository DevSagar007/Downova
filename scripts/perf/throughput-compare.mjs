#!/usr/bin/env node
// Unthrottled transfer throughput of two normal packages, alternating run by run (B3.1).
//
//   node scripts/perf/throughput-compare.mjs --a <package-dir> --b <package-dir> [--runs 5]
//        [--out perf-results/throughput-<ts>]
//
// Windows only. Each package's MSI is extracted once with `msiexec /a` (nothing is installed).
// Every run starts that package's bare service on a fresh data folder (no window: the UI is not
// on the transfer path), with no speed limit, and downloads from a local fixture in its own process
// (scripts/acceptance/limits-schedules-fixture.mjs, full speed, range-capable):
//   one_512:  one 512 MiB file;
//   six_128:  six 128 MiB files at once (global and Main limits set to 6 and read back).
// Order within a run alternates A/B and the workloads rotate. Measured: wall time from `dm add`
// until every file is completed (polled every 100 ms), bytes/s, and every SHA-256. Raw per-run
// JSON and a summary with medians and ranges are written; failed or mismatched runs are kept and
// excluded from the medians, with reasons. This measures the engine's unthrottled path on
// loopback; it says nothing about real networks.
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { readFirstJsonLine } from "../lib/first-line.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const dirs = { A: path.resolve(arg("--a")), B: path.resolve(arg("--b")) };
const runs = Number(arg("--runs", "5"));
const outDir = path.resolve(arg("--out", path.join("perf-results", `throughput-${Date.now()}`)));
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
if (process.platform !== "win32") {
  console.log("not run: Windows packages only");
  process.exit(2);
}
const sha256File = (p) => crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-tput-"));
const packages = {};
for (const [k, dir] of Object.entries(dirs)) {
  const info = JSON.parse(fs.readFileSync(path.join(dir, "build-info.json"), "utf8"));
  if (info.smoke_feature) throw new Error(`${k}: refusing a smoke-test package`);
  const msi = fs.readdirSync(dir).find((f) => f.endsWith(".msi"));
  const target = path.join(work, k);
  const ex = spawnSync("msiexec", ["/a", path.join(dir, msi), "/qn", `TARGETDIR=${target}`]);
  if (ex.status !== 0) throw new Error(`${k}: msiexec /a exited ${ex.status}`);
  const find = (d, name) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) {
        const f = find(p, name);
        if (f) return f;
      } else if (e.name.toLowerCase() === name) return p;
    }
    return null;
  };
  const service = find(target, "dm-service.exe");
  packages[k] = {
    dir,
    commit: info.source?.commit ?? info.git_commit,
    uncommitted_diff_sha256: info.source?.uncommitted_diff_sha256 ?? null,
    msi_sha256: crypto
      .createHash("sha256")
      .update(fs.readFileSync(path.join(dir, msi)))
      .digest("hex"),
    service,
    dm: path.join(path.dirname(service), "dm.exe"),
  };
}

const MIB = 1024 * 1024;
const fixture = spawn(
  process.execPath,
  [
    path.join(import.meta.dirname, "..", "acceptance", "limits-schedules-fixture.mjs"),
    `big:${512 * MIB}:7`,
    ...Array.from({ length: 6 }, (_, i) => `m${i + 1}:${128 * MIB}:${20 + i}`),
  ],
  { stdio: ["ignore", "pipe", "inherit"] },
);
const { base, sha256: expected } = await readFirstJsonLine(fixture, { timeoutMs: 300_000 });
const WORKLOADS = {
  one_512: ["big"],
  six_128: ["m1", "m2", "m3", "m4", "m5", "m6"],
};

async function once(key, workload, run) {
  const p = packages[key];
  const home = path.join(work, `home-${key}-${workload}-${run}`);
  const dl = path.join(work, `dl-${key}-${workload}-${run}`);
  fs.mkdirSync(dl, { recursive: true });
  const env = { ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl };
  const dm = (args) => {
    const r = spawnSync(p.dm, ["--data-dir", home, "--json", ...args], {
      env,
      encoding: "utf8",
      timeout: 30_000,
    });
    try {
      return JSON.parse(r.stdout);
    } catch {
      return null;
    }
  };
  const svc = spawn(p.service, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
  });
  const out = { package: key, workload, run, ok: false };
  try {
    for (let i = 0; i < 150 && !dm(["hello"])?.ok; i++) await sleep(200);
    const names = WORKLOADS[workload];
    if (names.length > 1) {
      dm(["concurrency-set", "6"]);
      dm(["queue-limit", "main", "6"]);
      out.limits = {
        global: dm(["concurrency"])?.data?.max_active_downloads,
        main: (dm(["queues"])?.data?.queues ?? []).find((q) => q.queue_id === "main")?.max_active,
      };
    }
    out.speed_limit = dm(["speed-limit"])?.data?.global_bytes_per_second ?? "unknown";
    const t0 = performance.now();
    const ids = names.map((n) => dm(["add", `${base}/${n}`])?.data?.job_id);
    let items = [];
    while (performance.now() - t0 < 600_000) {
      items = dm(["list"])?.data?.items ?? [];
      const mine = items.filter((j) => ids.includes(j.job_id));
      if (
        mine.length === ids.length &&
        mine.every((j) => ["completed", "failed", "cancelled"].includes(j.phase))
      )
        break;
      await sleep(100);
    }
    const seconds = (performance.now() - t0) / 1000;
    const mine = items.filter((j) => ids.includes(j.job_id));
    const bytes = names.length * (workload === "one_512" ? 512 : 128) * MIB;
    out.seconds = Number(seconds.toFixed(3));
    out.bytes_per_second = Math.round(bytes / seconds);
    out.phases = mine.map((j) => j.phase);
    out.hashes_ok = mine.every((j) => {
      const name = j.display_name;
      return j.phase === "completed" && sha256File(path.join(dl, name)) === expected[name];
    });
    out.ok = out.hashes_ok && mine.length === names.length;
    if (!out.ok) out.reason = `phases ${out.phases.join(",")}; hashes ${out.hashes_ok}`;
  } catch (e) {
    out.reason = String(e);
  } finally {
    dm(["shutdown"]);
    await sleep(500);
    try {
      svc.kill();
    } catch {
      /* already gone */
    }
    await sleep(500);
    fs.rmSync(dl, { recursive: true, force: true });
    fs.rmSync(home, { recursive: true, force: true });
  }
  fs.writeFileSync(
    path.join(outDir, `run${run}-${key}-${workload}.json`),
    JSON.stringify(out, null, 2),
  );
  console.log(JSON.stringify(out));
  return out;
}

const all = [];
for (let run = 1; run <= runs; run++) {
  const order = run % 2 ? ["A", "B"] : ["B", "A"];
  const wl = run % 2 ? ["one_512", "six_128"] : ["six_128", "one_512"];
  for (const w of wl) for (const k of order) all.push(await once(k, w, run));
}
fixture.kill();
const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const summary = {
  method:
    "bare packaged service, no window, unlimited, loopback fixture in its own process; A/B alternate within each run and the workload order rotates; wall time from add to all completed (100 ms polling)",
  host: `${os.type()} ${os.release()} ${os.arch()}; ${os.cpus()[0]?.model}; ${os.cpus().length} logical CPUs; ${Math.round(os.totalmem() / 2 ** 30)} GiB`,
  runs,
  packages: Object.fromEntries(
    Object.entries(packages).map(([k, p]) => [
      k,
      {
        dir: p.dir,
        commit: p.commit,
        uncommitted_diff_sha256: p.uncommitted_diff_sha256,
        msi_sha256: p.msi_sha256,
      },
    ]),
  ),
  results: {},
  excluded: all
    .filter((r) => !r.ok)
    .map((r) => ({ package: r.package, workload: r.workload, run: r.run, reason: r.reason })),
};
for (const w of Object.keys(WORKLOADS)) {
  for (const k of ["A", "B"]) {
    const ok = all.filter((r) => r.ok && r.workload === w && r.package === k);
    const rates = ok.map((r) => r.bytes_per_second);
    summary.results[`${w}:${k}`] = {
      n: ok.length,
      median_mib_s: rates.length ? Number((median(rates) / MIB).toFixed(1)) : null,
      min_mib_s: rates.length ? Number((Math.min(...rates) / MIB).toFixed(1)) : null,
      max_mib_s: rates.length ? Number((Math.max(...rates) / MIB).toFixed(1)) : null,
      seconds: ok.map((r) => r.seconds),
    };
  }
}
fs.writeFileSync(path.join(outDir, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
console.log(JSON.stringify(summary.results, null, 2));
try {
  fs.rmSync(work, { recursive: true, force: true });
} catch {
  /* best effort */
}
process.exit(summary.excluded.length ? 1 : 0);
