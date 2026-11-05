#!/usr/bin/env node
// B3.1 packaged exercise of CORE-06 (speed limits) and CORE-08 (queue schedules).
//
//   node scripts/acceptance/limits-schedules-acceptance.mjs --package-dir dist-packages/windows-x86_64
//
// Windows only. Extracts the NORMAL package's MSI with `msiexec /a` (nothing is installed), runs the
// packaged dm-service and dm with this run's own data and download folders, and a local fixture in
// this process. Rows:
//   L1 overall limit: one download at 500 KB/s takes 4 MB / 500 KB/s within ±10 %.
//   L2 per-download limit: 250 KB/s with the overall limit off, within ±10 %.
//   L3 the limits are durable: after a service restart `dm speed-limit` reports the saved value.
//   S1 outside its window a scheduled queue's download waits with schedule_window and receives nothing.
//   S2 the schedule is shown by `dm queues` with its state.
//   S3 moving the window over now admits it, and it completes byte-exact.
//   S4 the schedule is durable across a restart; `off` removes it.
// Results: acceptance-results/<label>-limits-schedules/results.json. No GUI step (not run here).
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const packageDir = path.resolve(arg("--package-dir") ?? "dist-packages/windows-x86_64");
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
const outDir = path.resolve("acceptance-results", `${buildInfo.label}-limits-schedules`);
fs.mkdirSync(outDir, { recursive: true });
const results = {
  started_at: new Date().toISOString(),
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  rows: [],
};
const record = (id, status, detail = {}) => {
  results.rows.push({ id, status, ...detail });
  console.log(`${status.toUpperCase().padEnd(7)} ${id} ${JSON.stringify(detail)}`);
};
const finish = (code) => {
  results.finished_at = new Date().toISOString();
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  process.exit(code);
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "this script drives Windows packages only" });
  finish(0);
}
if (buildInfo.smoke_feature) throw new Error("refusing a smoke-test package: use the normal build");
results.build_info = { source: buildInfo.source, built_at: buildInfo.built_at };
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
results.msi_sha256 = sha256(fs.readFileSync(path.join(packageDir, msi)));

const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-b31-"));
const ex = spawnSync("msiexec", [
  "/a",
  path.join(packageDir, msi),
  "/qn",
  `TARGETDIR=${path.join(work, "app")}`,
]);
if (ex.status !== 0) throw new Error(`msiexec /a exited ${ex.status}`);
function findFile(dir, name) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name.toLowerCase() === name) return p;
  }
  return null;
}
const serviceExe = findFile(path.join(work, "app"), "dm-service.exe");
const dmExe = path.join(path.dirname(serviceExe), "dm.exe");
const home = path.join(work, "data");
const dl = path.join(work, "downloads");
fs.mkdirSync(dl, { recursive: true });
const env = { ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl };

// ------------------------------------------------------------------ fixture
const SIZE = 4_000_000;
const body = crypto.randomBytes(SIZE);
const bodyHash = sha256(body);
const server = http.createServer((req, res) => {
  const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
  const headers = {
    "Accept-Ranges": "bytes",
    ETag: '"b31-fixture"',
    "Last-Modified": "Wed, 04 Nov 2026 00:00:00 GMT",
  };
  if (range) {
    const start = Number(range[1]);
    const end = range[2] ? Math.min(Number(range[2]), SIZE - 1) : SIZE - 1;
    res.writeHead(206, {
      ...headers,
      "Content-Length": end - start + 1,
      "Content-Range": `bytes ${start}-${end}/${SIZE}`,
    });
    res.end(body.subarray(start, end + 1));
  } else {
    res.writeHead(200, { ...headers, "Content-Length": SIZE });
    res.end(body);
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

// ------------------------------------------------------------------ service and dm
function dm(args) {
  const r = spawnSync(dmExe, ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
    timeout: 30_000,
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null, stdout: r.stdout, stderr: r.stderr };
  }
}
function dmText(args) {
  return spawnSync(dmExe, ["--data-dir", home, ...args], { env, encoding: "utf8", timeout: 30_000 })
    .stdout;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(what, ok, ms) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (ok()) return;
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}`);
}
async function startService() {
  const svc = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
    detached: true,
  });
  svc.unref();
  await until("the service", () => dm(["hello"]).code === 0, 30_000);
}
async function stopService() {
  dm(["shutdown"]);
  await until("the service to stop", () => dm(["hello"]).code !== 0, 30_000);
}
const status = (id) => dm(["get", id]).json?.data;
async function timedDownload(name, extra) {
  const added = dm(["add", `${base}/${name}`, ...extra]);
  if (added.code !== 0) throw new Error(`add failed: ${JSON.stringify(added)}`);
  const id = added.json.data.job_id;
  const t0 = Date.now();
  await until(`${name} to complete`, () => status(id)?.phase === "completed", 60_000);
  const seconds = (Date.now() - t0) / 1000;
  const file = path.join(dl, status(id).display_name);
  return {
    id,
    seconds,
    rate: Math.round(SIZE / seconds),
    exact: sha256(fs.readFileSync(file)) === bodyHash,
  };
}
const within10 = (rate, limit) => Math.abs(rate - limit) / limit <= 0.1;

let failed = 0;
const row = (id, ok, detail) => {
  if (!ok) failed += 1;
  record(id, ok ? "passed" : "failed", detail);
};
try {
  await startService();
  results.hello = dm(["hello"]).json?.data?.backend;

  // L1: overall limit.
  const set = dm(["speed-limit-set", "500k"]);
  const l1 = await timedDownload("l1.bin", []);
  row("L1", set.code === 0 && within10(l1.rate, 500_000) && l1.exact, { limit: 500_000, ...l1 });

  // L2: per-download limit with the overall limit off.
  dm(["speed-limit-set", "off"]);
  const l2 = await timedDownload("l2.bin", ["--speed-limit", "250k"]);
  row("L2", within10(l2.rate, 250_000) && l2.exact, { limit: 250_000, ...l2 });

  // S1/S2: a queue whose only window is not now.
  dm(["speed-limit-set", "2m"]);
  const q = dm(["queue-create", "Night"]).json?.data?.queue_id;
  const h = new Date().getUTCHours();
  const hh = (n) => String(((n % 24) + 24) % 24).padStart(2, "0") + ":00";
  const later = dm(["queue-schedule", q, "UTC", `daily@${hh(h + 3)}-${hh(h + 4)}`]);
  const added = dm(["add", `${base}/s1.bin`, "--queue", q]);
  const sid = added.json?.data?.job_id;
  await sleep(3000);
  const waiting = status(sid);
  row(
    "S1",
    later.code === 0 &&
      waiting?.blockers?.includes("schedule_window") &&
      waiting.downloaded_bytes === "0",
    {
      blockers: waiting?.blockers,
      phase: waiting?.phase,
      downloaded_bytes: waiting?.downloaded_bytes,
    },
  );
  const listing = dmText(["queues"]);
  row("S2", /schedule on \(UTC\)/.test(listing) && /waiting for its next window/.test(listing), {
    queues: listing,
  });

  // S3: the window moves over now.
  const now = dm(["queue-schedule", q, "UTC", `daily@${hh(h - 1)}-${hh(h + 2)}`, "catch-up"]);
  await until("the scheduled download", () => status(sid)?.phase === "completed", 60_000);
  const done = status(sid);
  row(
    "S3",
    now.code === 0 && sha256(fs.readFileSync(path.join(dl, done.display_name))) === bodyHash,
    {
      blockers: done.blockers,
    },
  );

  // L3/S4: durable across a restart; then off.
  await stopService();
  await startService();
  const limit = dm(["speed-limit"]).json?.data;
  row("L3", limit?.global_bytes_per_second === "2000000", { after_restart: limit });
  const queues = dm(["queues"]).json?.data?.queues ?? [];
  const night = queues.find((x) => x.queue_id === q);
  const off = dm(["queue-schedule", q, "off"]);
  const removed = (dm(["queues"]).json?.data?.queues ?? []).find((x) => x.queue_id === q);
  row(
    "S4",
    night?.schedule?.missed_window === "catch_up" &&
      night?.schedule_state?.in_window === true &&
      off.code === 0 &&
      removed &&
      !removed.schedule,
    {
      after_restart: { schedule: night?.schedule, schedule_state: night?.schedule_state },
    },
  );
  record("GUI", "not run", {
    reason: "the Options, details and Manage queues UI is covered by vitest only in this batch",
  });
} catch (e) {
  failed += 1;
  record("error", "failed", { message: String(e?.message ?? e) });
} finally {
  try {
    await stopService();
  } catch {
    /* already stopped */
  }
  server.close();
}
finish(failed === 0 ? 0 : 1);
