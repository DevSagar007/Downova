#!/usr/bin/env node
// Release-build resource measurements (REL-04 evidence; docs/testing.md §9).
//
//   node scripts/perf/measure.mjs --package-dir dist-packages/windows-x86_64 [--runs 5]
//        [--sample-s 10] [--out perf-results/<label>] [--scenarios a,b,…]
//
// Windows only for now (process accounting uses Win32_Process; other OSes report "not run").
//
// What runs: the NORMAL release package (never the smoke build), extracted from its MSI with
// `msiexec /a` into a work folder — nothing is installed. Every run uses its own data, download
// and WebView2 user-data folders (DOWNOVA_HOME, DOWNOVA_DOWNLOAD_DIR, WEBVIEW2_USER_DATA_FOLDER),
// so a real installation and its data are never touched. If another Downova desktop is running,
// the run stops (the single-instance lock would make the measurement meaningless); nothing is
// killed except processes this tool started.
//
// Scenarios (order rotated every run):
//   idle_visible    window shown, empty history, service idle
//   idle_hidden     the same after closing the window (hide to tray, WM_CLOSE)
//   ui_closed       desktop process tree ended; the detached service keeps running
//   active_1        one transfer from a local fixture (separate process), window visible
//   active_3        three transfers
//   active_6        six transfers with the global limit AND the Main queue limit set to 6 (B2.32)
//
// Active scenarios (B2.32): each scenario's files and pacing keep every transfer running through
// the whole sample window (about 30 s of transfer at the host's 8 request permits; a single file
// uses its 4 segments), and the run records proof of activity — every download gained bytes
// during the window and none finished before it ended — then waits for completion and records the
// completion time, the bytes transferred and every file's SHA-256 against the fixture's. Before
// B2.32, active_1 at 25 ms per 64 KiB on 4 segments finished about 6.5 s after it was added, so
// its 2–12 s window was partly idle.
//   history_1k      1,000 finished records (seeded into the database before start)
//   history_10k     10,000 finished records; also UI latencies (search, scroll) over CDP
//   service_start   no desktop: dm-service started twice on one fresh data folder; the first
//                   start creates and migrates the database, the second opens it (B2.24)
//   cycles          window hidden and reopened 5 times, then the service restarted 3 times (the
//                   window reattaches on its own); memory and JS heap before and after (B2.24)
//
// Startup (every desktop scenario, B2.24), all from the desktop spawn on the same clock:
//   service_ready_ms        `dm hello` answers (includes the service's own start and migration)
//   ui_dom_ready_ms         the page's DOMContentLoaded (always available)
//   ui_first_paint_ms       the WebView's first contentful paint (performance.timeOrigin + FCP); WebView2
//                           often records none when the window first paints hidden, then null
//   list_ready_ms           the list shows rows or its loaded-empty hint; when that happened before
//                           DevTools attached, the attach time is reported with list_ready_bound
//                           "upper" instead of "exact"
//
// --diagnose (separate runs; never mixed into resource medians, because instrumentation costs
// memory and CPU): CDP Performance metrics at the start and end of the sample window (JS heap,
// DOM nodes, listeners, layouts, style recalcs, script time), the JS heap after a forced GC, and
// DOM mutation and animation-frame counts in the list during the window.
//
// Per scenario: service-ready latency (desktop spawn → `dm hello` answers), then CPU and memory
// sampled over --sample-s seconds for every owned process: the desktop, its WebView2 processes
// (descendants) and the detached service. Memory: working set (includes shared pages, so a sum
// over-counts) and private bytes (PrivatePageCount, private commit; summable). Both are reported
// per process and aggregated; neither is "RAM used" by itself.
//
// UI latencies (history_10k) are measured in the real WebView through its DevTools endpoint
// (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port) with language-independent
// selectors: rendered row count, search-to-paint latency (20 queries), scroll-to-paint latency.
//
// Output: raw JSON per run plus samples.csv and summary.json (medians; p95 only with ≥ 20
// observations), with OS/runtime versions, the package's build-info (commit + uncommitted diff)
// and this tool's settings.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { seedHistory } from "../lib/seed-history.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(arg("--package-dir", "dist-packages/windows-x86_64"));
const runs = Number(arg("--runs", "5"));
const sampleS = Number(arg("--sample-s", "10"));
const label = arg("--label", path.basename(packageDir));
const outDir = path.resolve(arg("--out", path.join("perf-results", `${label}-${Date.now()}`)));
/** Per active scenario: files, size and per-64 KiB pacing (about 30 s of transfer). */
const ACTIVE = {
  active_1: { n: 1, sizeMiB: 64, slow: 117 },
  active_3: { n: 3, sizeMiB: 64, slow: 78 },
  active_6: { n: 6, sizeMiB: 32, slow: 78 },
};
const ALL = [
  "service_start",
  "cycles",
  "idle_visible",
  "idle_hidden",
  "ui_closed",
  "active_1",
  "active_3",
  "active_6",
  "history_1k",
  "history_10k",
];
const scenarios = (arg("--scenarios") ?? ALL.join(",")).split(",");
const diagnose = argv.includes("--diagnose");
// Starting rotation (lets two packages alternate one run at a time and still rotate: B2.24).
const rotate = Number(arg("--rotate", "0"));
// Active transfers must outlast the sample window (25 ms per 64 KiB ≈ 26 s per file).
const slowMs = Number(arg("--slow-ms", "25"));
// Extra WebView2 browser arguments for experiments (e.g. --disable-gpu); recorded in the summary.
// Never the product's configuration: results with it are labelled as experiments.
const webviewArgs = arg("--webview-args", "");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
fs.mkdirSync(outDir, { recursive: true });

if (process.platform !== "win32") {
  fs.writeFileSync(
    path.join(outDir, "summary.json"),
    JSON.stringify(
      { status: "not run", reason: "process accounting is implemented for Windows only" },
      null,
      2,
    ),
  );
  console.log("not run: Windows only");
  process.exit(0);
}

// ------------------------------------------------------------------ package
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
if (buildInfo.smoke_feature)
  throw new Error("refusing a smoke-test package: measure the normal build");
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-perf-"));
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
const setup = fs.readdirSync(packageDir).find((f) => f.endsWith("-setup.exe"));
const appDir = path.join(work, "app");
const ex = spawnSync("msiexec", ["/a", path.join(packageDir, msi), "/qn", `TARGETDIR=${appDir}`]);
if (ex.status !== 0) throw new Error(`msiexec /a exited ${ex.status}`);
function findFile(dir, name) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name.toLowerCase() === name.toLowerCase()) return p;
  }
  return null;
}
const serviceExe = findFile(appDir, "dm-service.exe");
const binDir = path.dirname(serviceExe);
const desktopExe = fs
  .readdirSync(binDir)
  .map((n) => path.join(binDir, n))
  .find(
    (p) => /\.exe$/i.test(p) && !/^(dm|dm-service|dm-browser-host)\.exe$/i.test(path.basename(p)),
  );
const dmExe = path.join(binDir, "dm.exe");
const dirSize = (d) =>
  fs.readdirSync(d, { withFileTypes: true }).reduce((s, e) => {
    const p = path.join(d, e.name);
    return s + (e.isDirectory() ? dirSize(p) : fs.statSync(p).size);
  }, 0);
const sizes = {
  installer_bytes: fs.statSync(path.join(packageDir, setup)).size,
  msi_bytes: fs.statSync(path.join(packageDir, msi)).size,
  // The extracted MSI contents: the files an installation places (no WebView2 runtime, which is
  // shared and system-provided).
  installed_bytes: dirSize(binDir),
};

// ------------------------------------------------------------------ processes
function processTable() {
  const ps = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,WorkingSetSize,PrivatePageCount,KernelModeTime,UserModeTime,CommandLine | ConvertTo-Json -Compress",
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  return JSON.parse(ps.stdout);
}
function ownedProcesses(table, roots, webviewFolder) {
  const kids = new Map();
  for (const p of table) {
    if (!kids.has(p.ParentProcessId)) kids.set(p.ParentProcessId, []);
    kids.get(p.ParentProcessId).push(p);
  }
  const out = new Map();
  const visit = (pid) => {
    for (const c of kids.get(pid) ?? []) {
      if (!out.has(c.ProcessId)) {
        out.set(c.ProcessId, c);
        visit(c.ProcessId);
      }
    }
  };
  for (const pid of roots.filter(Boolean)) {
    const self = table.find((p) => p.ProcessId === pid);
    if (self) out.set(pid, self);
    visit(pid);
  }
  // WebView2 processes are children of msedgewebview2's broker; match our user-data folder too.
  for (const p of table)
    if (webviewFolder && (p.CommandLine ?? "").includes(webviewFolder) && !out.has(p.ProcessId))
      out.set(p.ProcessId, p);
  return [...out.values()];
}
const cores = os.cpus().length;
async function sample(roots, webviewFolder, seconds) {
  const t0 = process.hrtime.bigint();
  const a = ownedProcesses(processTable(), roots, webviewFolder);
  await sleep(seconds * 1000);
  const t1 = process.hrtime.bigint();
  const table = processTable();
  const b = ownedProcesses(table, roots, webviewFolder);
  const wallMs = Number(t1 - t0) / 1e6;
  const cpuOf = (p) => (Number(p.KernelModeTime) + Number(p.UserModeTime)) / 1e4; // 100 ns → ms
  const before = new Map(a.map((p) => [p.ProcessId, cpuOf(p)]));
  const procs = b.map((p) => ({
    pid: p.ProcessId,
    name: p.Name,
    role:
      p.Name.toLowerCase() === "dm-service.exe"
        ? "service"
        : p.Name.toLowerCase().startsWith("msedgewebview2")
          ? "webview"
          : "desktop",
    // Chromium process type (browser, renderer, gpu-process, utility, crashpad-handler) for WebView2.
    type: /--type=([a-z-]+)/.exec(p.CommandLine ?? "")?.[1] ?? null,
    working_set_bytes: Number(p.WorkingSetSize),
    private_bytes: Number(p.PrivatePageCount),
    // CPU of processes alive for the whole interval; a process started mid-interval counts from 0.
    cpu_ms: cpuOf(p) - (before.get(p.ProcessId) ?? 0),
  }));
  const sum = (k) => procs.reduce((s, p) => s + p[k], 0);
  return {
    wall_ms: wallMs,
    process_count: procs.length,
    cpu_percent_of_machine: (sum("cpu_ms") / (wallMs * cores)) * 100,
    cpu_ms: sum("cpu_ms"),
    private_bytes_total: sum("private_bytes"),
    working_set_bytes_total_overcounted: sum("working_set_bytes"),
    by_role: Object.fromEntries(
      ["desktop", "webview", "service"].map((r) => {
        const rs = procs.filter((p) => p.role === r);
        return [
          r,
          {
            count: rs.length,
            private_bytes: rs.reduce((s, p) => s + p.private_bytes, 0),
            working_set_bytes: rs.reduce((s, p) => s + p.working_set_bytes, 0),
            cpu_ms: rs.reduce((s, p) => s + p.cpu_ms, 0),
          },
        ];
      }),
    ),
    processes: procs,
  };
}

// ------------------------------------------------------------------ app control
function dm(home, dl, args) {
  const r = spawnSync(dmExe, ["--data-dir", home, "--json", ...args], {
    env: { ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl },
    encoding: "utf8",
    timeout: 30_000,
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null };
  }
}
function sha256File(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function servicePid(home) {
  try {
    return JSON.parse(fs.readFileSync(path.join(home, "run", "endpoint.json"), "utf8")).pid;
  } catch {
    return null;
  }
}
function killTree(pid) {
  if (pid) spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"]);
}
function otherDownovaRunning() {
  return processTable().some(
    (p) =>
      /^(downova|dm-desktop)\.exe$/i.test(p.Name) &&
      !(p.CommandLine ?? "").toLowerCase().includes(work.toLowerCase()),
  );
}
let nextPort = 9400;

/**
 * Seeds `count` finished jobs straight into a fresh database (created by the service itself, then
 * stopped). Test data only: lives in this run's own data folder, never in a product build.
 */
async function seedDatabase(home, dl, count) {
  const env = { ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl };
  const svc = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
  });
  for (let i = 0; i < 100 && dm(home, dl, ["hello"]).code !== 0; i++) await sleep(100);
  dm(home, dl, ["shutdown"]);
  for (let i = 0; i < 100 && svc.exitCode === null; i++) await sleep(100);
  seedHistory(path.join(home, "downova.sqlite3"), dl, count);
}

/** CDP over the WebView's DevTools endpoint. */
async function cdp(port) {
  let page;
  for (let i = 0; i < 100 && !page; i++) {
    try {
      const list = await (
        await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(5000) })
      ).json();
      page = list.find((t) => t.type === "page");
    } catch {
      /* not up yet */
    }
    if (!page) await sleep(200);
  }
  if (!page) throw new Error("no WebView DevTools page");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => {
    const timer = setTimeout(() => j(new Error("DevTools socket did not open in 15 s")), 15_000);
    ws.onopen = () => {
      clearTimeout(timer);
      r();
    };
    ws.onerror = (e) => {
      clearTimeout(timer);
      j(e);
    };
  });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && waiting.has(msg.id)) {
      waiting.get(msg.id)(msg);
      waiting.delete(msg.id);
    }
  };
  const send = (method, params = {}, ms = 60_000) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => {
        waiting.delete(n);
        reject(new Error(`DevTools ${method} timed out`));
      }, ms);
      waiting.set(n, (msg) => {
        clearTimeout(timer);
        return msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
      });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const evaluate = (expression, ms = 150_000) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => {
        waiting.delete(n);
        reject(new Error(`DevTools call timed out after ${ms} ms (page unresponsive)`));
      }, ms);
      waiting.set(n, (msg) => {
        clearTimeout(timer);
        return msg.result?.exceptionDetails
          ? reject(new Error(msg.result.exceptionDetails.text))
          : resolve(msg.result?.result?.value);
      });
      ws.send(
        JSON.stringify({
          id: n,
          method: "Runtime.evaluate",
          params: { expression, awaitPromise: true, returnByValue: true },
        }),
      );
    });
  return { evaluate, send, close: () => ws.close() };
}

/** Search and scroll latencies in the real page (language-independent selectors). */
const UI_PROBE = `(async () => {
  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const until = async (fn, ms) => { const t0 = performance.now(); while (performance.now() - t0 < ms) { if (fn()) return true; await new Promise((r) => setTimeout(r, 20)); } return false; };
  await until(() => document.querySelectorAll("[data-job-id]").length > 0, 60000);
  const rows = () => document.querySelectorAll("[data-job-id]").length;
  const domNodes = document.getElementsByTagName("*").length;
  const rendered = rows();
  const input = document.querySelector("#download-search");
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  // Bounded: after 60 s the probe stops and reports what it measured (a slow page is evidence).
  const budget = performance.now() + 60000;
  const search = [];
  const queries = ["report", "", "দলিল", "", "9999", "", "video-lecture-12", "", "zip", ""];
  for (let k = 0; k < 2; k++)
    for (const q of queries) {
      if (performance.now() > budget) break;
      const t0 = performance.now();
      setValue.call(input, q);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await frame();
      search.push(performance.now() - t0);
    }
  const scroller = document.querySelector(".download-table__scroll");
  const scroll = [];
  for (let k = 0; k < 20; k++) {
    if (performance.now() > budget) break;
    const t0 = performance.now();
    scroller.scrollTop = (k % 2 ? 0 : scroller.scrollHeight * ((k + 1) / 21));
    await frame();
    scroll.push(performance.now() - t0);
  }
  return { rendered_rows: rendered, dom_nodes: domNodes, search_ms: search, scroll_ms: scroll, budget_exhausted: performance.now() > budget };
})()`;

/** CDP Performance metrics as an object (diagnostics only). */
async function metrics(page) {
  const r = await page.send("Performance.getMetrics");
  return Object.fromEntries(r.metrics.map((m) => [m.name, m.value]));
}
const PICK = [
  "JSHeapUsedSize",
  "JSHeapTotalSize",
  "Nodes",
  "JSEventListeners",
  "Documents",
  "LayoutCount",
  "RecalcStyleCount",
  "ScriptDuration",
  "TaskDuration",
];
const pick = (m) => Object.fromEntries(PICK.map((k) => [k, m[k]]));
/** In-page counters over the sample window: DOM mutations in the list and animation frames. */
const COUNTERS_START = `(() => {
  const c = { mutations: 0, records: 0, frames: 0, stop: false };
  const target = document.querySelector("#download-list") ?? document.body;
  const mo = new MutationObserver((rs) => { c.records += rs.length; c.mutations += 1; });
  mo.observe(target, { subtree: true, childList: true, characterData: true, attributes: true });
  const raf = () => { if (c.stop) return; c.frames += 1; requestAnimationFrame(raf); };
  requestAnimationFrame(raf);
  window.__dmPerf = { c, mo };
  return true;
})()`;
const COUNTERS_STOP = `(() => {
  const p = window.__dmPerf; p.c.stop = true; p.mo.disconnect(); delete window.__dmPerf;
  return { mutation_callbacks: p.c.mutations, mutation_records: p.c.records, animation_frames: p.c.frames,
    rows_rendered: document.querySelectorAll("[data-job-id]").length };
})()`;
/** Page-side startup marks (epoch ms on the machine clock). */
const STARTUP = `(async () => {
  const listShown = () => document.querySelector("[data-job-id], .download-table__hint");
  const already = !!listShown();
  const t0 = performance.now();
  while (!listShown() && performance.now() - t0 < 60000) await new Promise((r) => setTimeout(r, 10));
  const fcp = performance.getEntriesByType("paint").find((e) => e.name === "first-contentful-paint");
  return {
    fcp_epoch: fcp ? performance.timeOrigin + fcp.startTime : null,
    dom_ready_epoch: performance.timing.domContentLoadedEventEnd || null,
    list_epoch: listShown() ? Date.now() : null,
    list_already_shown: already,
  };
})()`;

/** First and second start of the bare service on one data folder (no desktop, no WebView). */
async function serviceStart(runNo) {
  const dir = path.join(work, `${runNo}-service_start`);
  const home = path.join(dir, "data");
  const dl = path.join(dir, "downloads");
  fs.mkdirSync(dl, { recursive: true });
  const env = { ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl };
  const once = async () => {
    const t0 = performance.now();
    const svc = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
      env,
      stdio: "ignore",
    });
    let ready = null;
    for (let i = 0; i < 600; i++) {
      if (dm(home, dl, ["hello"]).code === 0) {
        ready = performance.now() - t0;
        break;
      }
      await sleep(20);
    }
    const s = ready === null ? null : await sample([svc.pid], null, 2);
    dm(home, dl, ["shutdown"]);
    for (let i = 0; i < 100 && svc.exitCode === null; i++) await sleep(100);
    if (svc.exitCode === null) killTree(svc.pid);
    return { ready_ms: ready, private_bytes: s?.private_bytes_total ?? null };
  };
  const first = await once();
  const second = await once();
  return {
    scenario: "service_start",
    run: runNo,
    status: first.ready_ms !== null && second.ready_ms !== null ? "ok" : "error",
    method:
      "spawn → first successful `dm hello` (polled every 20 ms; each poll starts dm.exe, which adds its own start time)",
    first_start_ms: first.ready_ms,
    second_start_ms: second.ready_ms,
    first_private_bytes: first.private_bytes,
    second_private_bytes: second.private_bytes,
  };
}

async function runScenario(name, runNo) {
  if (name === "service_start") return serviceStart(runNo);
  const dir = path.join(work, `${runNo}-${name}`);
  const home = path.join(dir, "data");
  const dl = path.join(dir, "downloads");
  const wv = path.join(dir, "webview");
  fs.mkdirSync(dl, { recursive: true });
  if (otherDownovaRunning())
    return { scenario: name, status: "not run", reason: "another Downova desktop is running" };
  if (name.startsWith("history_"))
    await seedDatabase(home, dl, name === "history_1k" ? 1000 : 10000);
  const port = nextPort++;
  const env = {
    ...process.env,
    DOWNOVA_HOME: home,
    DOWNOVA_DOWNLOAD_DIR: dl,
    WEBVIEW2_USER_DATA_FOLDER: wv,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} ${webviewArgs}`.trim(),
  };
  const t0 = performance.now();
  const spawnEpoch = Date.now();
  const desktop = spawn(desktopExe, [], { env, stdio: "ignore" });
  let readyMs = null;
  for (let i = 0; i < 600; i++) {
    if (dm(home, dl, ["hello"]).code === 0) {
      readyMs = performance.now() - t0;
      break;
    }
    await sleep(100);
  }
  const result = { scenario: name, run: runNo, status: "ok", service_ready_ms: readyMs };
  let fixture = null;
  let active = null;
  try {
    // The page is attached before anything is measured (first paint done).
    const page = await cdp(port);
    const firstPaint = await page.evaluate(
      `new Promise((r) => { const t0 = performance.now(); const check = () => document.querySelector("#download-list") ? r(performance.timing.domContentLoadedEventEnd - performance.timing.navigationStart) : setTimeout(check, 50); check(); })`,
    );
    result.webview_dom_ready_ms = firstPaint;
    const marks = await page.evaluate(STARTUP);
    result.ui_first_paint_ms = marks.fcp_epoch === null ? null : marks.fcp_epoch - spawnEpoch;
    result.ui_dom_ready_ms =
      marks.dom_ready_epoch === null ? null : marks.dom_ready_epoch - spawnEpoch;
    result.list_ready_ms = marks.list_epoch === null ? null : marks.list_epoch - spawnEpoch;
    result.list_ready_bound = marks.list_already_shown ? "upper" : "exact";
    if (name === "history_1k" || name === "history_10k") {
      const tl = performance.now();
      const shown = await page.evaluate(
        `new Promise((r) => { const t0 = performance.now(); const tick = () => document.querySelectorAll("[data-job-id]").length > 0 ? r(performance.now() - t0) : setTimeout(tick, 20); tick(); })`,
      );
      result.list_first_rows_ms = shown;
      result.list_wait_ms = performance.now() - tl;
    }
    if (ACTIVE[name]) {
      const { n, sizeMiB, slow } = ACTIVE[name];
      // A local fixture in its own process (excluded from the measurement): slow, steady bodies.
      fixture = spawn(
        process.execPath,
        [
          path.join(import.meta.dirname, "fixture-process.mjs"),
          "--count",
          String(n),
          "--size-mib",
          String(sizeMiB),
        ],
        { stdio: ["ignore", "pipe", "ignore"] },
      );
      const served = JSON.parse(
        await new Promise((r) => fixture.stdout.once("data", (d) => r(String(d).trim()))),
      );
      // Six at once needs both limits (the smaller one applies): set and read them back.
      if (n > 3) {
        dm(home, dl, ["concurrency-set", String(n)]);
        dm(home, dl, ["queue-limit", "main", String(n)]);
      }
      const limits = {
        global: dm(home, dl, ["concurrency"]).json?.data?.max_active_downloads ?? null,
        main:
          (dm(home, dl, ["queues"]).json?.data?.queues ?? []).find((q) => q.queue_id === "main")
            ?.max_active ?? null,
      };
      const pace = Number(arg("--slow-ms", "0")) || slow;
      const addedAt = performance.now();
      const ids = [];
      for (let k = 0; k < n; k++)
        ids.push(dm(home, dl, ["add", `${served.base}/f${k}.bin?slow=${pace}`]).json?.data?.job_id);
      active = { ids, addedAt, expected: served.sha256, limits, n, sizeMiB, pace };
      await sleep(2000);
    }
    if (name === "idle_hidden") {
      spawnSync("taskkill", ["/PID", String(desktop.pid)]); // WM_CLOSE → hide to tray
      await sleep(3000);
    }
    if (name === "ui_closed") {
      // Only the desktop process: the service is its child process tree-wise (\`/T\` would end it
      // too), but it is meant to outlive the window. WebView2 processes end with their host.
      spawnSync("taskkill", ["/PID", String(desktop.pid), "/F"]);
      await sleep(3000);
    }
    if (name === "cycles") {
      // Hide and reopen (a second launch shows the running instance), then service restarts.
      await page.send("Performance.enable");
      await page.send("HeapProfiler.collectGarbage");
      const before = await metrics(page);
      const first = await sample([desktop.pid, servicePid(home)], wv, 2);
      const steps = [];
      for (let k = 0; k < 5; k++) {
        spawnSync("taskkill", ["/PID", String(desktop.pid)]); // WM_CLOSE → hide to tray
        await sleep(1500);
        spawn(desktopExe, [], { env, stdio: "ignore" }); // single instance: shows the window
        await sleep(2000);
        steps.push({ step: `reopen ${k + 1}` });
      }
      for (let k = 0; k < 3; k++) {
        dm(home, dl, ["shutdown"]);
        await sleep(1500);
        const svc = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
          env,
          stdio: "ignore",
          detached: true,
        });
        svc.unref();
        // The window reattaches on its own (attach-only probes); a hidden spacer-free list with
        // its loaded hint and no warning banner means it is live again.
        let back = null;
        const tb = performance.now();
        for (let i = 0; i < 150 && back === null; i++) {
          const live = await page.evaluate(
            `!!document.querySelector(".download-table__hint") && !document.querySelector(".app-window__notice")`,
          );
          if (live) back = performance.now() - tb;
          else await sleep(200);
        }
        steps.push({ step: `service restart ${k + 1}`, window_reattached_ms: back });
      }
      await page.send("HeapProfiler.collectGarbage");
      const after = await metrics(page);
      const last = await sample([desktop.pid, servicePid(home)], wv, 2);
      result.cycles = {
        steps,
        js_heap_after_gc: [before.JSHeapUsedSize, after.JSHeapUsedSize],
        listeners: [before.JSEventListeners, after.JSEventListeners],
        nodes: [before.Nodes, after.Nodes],
        documents: [before.Documents, after.Documents],
        private_bytes_first: first.private_bytes_total,
        private_bytes_after: last.private_bytes_total,
      };
    }
    const roots = name === "ui_closed" ? [servicePid(home)] : [desktop.pid, servicePid(home)];
    if (diagnose && name !== "ui_closed") {
      await page.send("Performance.enable");
      result.diag = { start: pick(await metrics(page)) };
      await page.evaluate(COUNTERS_START);
    }
    const bytesOf = () => {
      const items = dm(home, dl, ["list"]).json?.data?.items ?? [];
      return Object.fromEntries(
        (active?.ids ?? []).map((id) => {
          const j = items.find((x) => x.job_id === id);
          return [id, { bytes: Number(j?.downloaded_bytes ?? 0), phase: j?.phase ?? null }];
        }),
      );
    };
    const atStart = active ? bytesOf() : null;
    result.sample = await sample(roots, wv, sampleS);
    if (active) {
      // Proof of activity over the window, then completion and integrity (outside the sample).
      const atEnd = bytesOf();
      result.activity = {
        limits: active.limits,
        files: active.n,
        size_mib: active.sizeMiB,
        slow_ms: active.pace,
        every_download_gained_bytes: active.ids.every((id) => atEnd[id].bytes > atStart[id].bytes),
        finished_before_window_end: active.ids.filter((id) => atEnd[id].phase === "completed")
          .length,
        bytes_in_window: active.ids.reduce((s, id) => s + (atEnd[id].bytes - atStart[id].bytes), 0),
      };
      const deadline = performance.now() + 240_000;
      let items = [];
      while (performance.now() < deadline) {
        items = dm(home, dl, ["list"]).json?.data?.items ?? [];
        const mine = active.ids.map((id) => items.find((x) => x.job_id === id));
        if (mine.every((j) => j && ["completed", "failed", "cancelled"].includes(j.phase))) break;
        await sleep(500);
      }
      const doneAt = performance.now();
      const files = active.ids.map((id, k) => {
        const j = items.find((x) => x.job_id === id);
        const file = path.join(dl, j?.display_name ?? `f${k}.bin`);
        const ok =
          j?.phase === "completed" &&
          fs.existsSync(file) &&
          sha256File(file) === active.expected[`f${k}.bin`];
        return {
          phase: j?.phase ?? null,
          bytes: Number(j?.downloaded_bytes ?? 0),
          sha256_matches: ok,
        };
      });
      result.completion = {
        all_completed: files.every((x) => x.phase === "completed"),
        completion_ms_from_add: Math.round(doneAt - active.addedAt),
        bytes_transferred: files.reduce((s, x) => s + x.bytes, 0),
        all_sha256_match: files.every((x) => x.sha256_matches),
        files,
      };
    }
    if (diagnose && name !== "ui_closed") {
      result.diag.end = pick(await metrics(page));
      result.diag.window = await page.evaluate(COUNTERS_STOP);
      await page.send("HeapProfiler.collectGarbage");
      result.diag.after_gc = pick(await metrics(page));
    }
    if (name === "history_10k") result.ui = await page.evaluate(UI_PROBE);
    if (name === "history_1k") result.ui = await page.evaluate(UI_PROBE);
    page.close();
  } catch (e) {
    result.status = "error";
    result.error = String(e?.message ?? e);
  } finally {
    if (fixture) fixture.kill();
    killTree(desktop.pid);
    killTree(servicePid(home));
    await sleep(1000);
  }
  return result;
}

// ------------------------------------------------------------------ main
const versions = {
  os: `${os.type()} ${os.release()} ${os.arch()}`,
  cpu: os.cpus()[0]?.model,
  cores,
  total_memory_bytes: os.totalmem(),
  node: process.version,
  webview2: (() => {
    try {
      return execFileSync(
        "reg",
        [
          "query",
          "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
          "/v",
          "pv",
        ],
        { encoding: "utf8" },
      ).match(/pv\s+REG_SZ\s+(\S+)/)?.[1];
    } catch {
      return null;
    }
  })(),
};
const all = [];
for (let r = 0; r < runs; r++) {
  // Rotated order: each scenario takes every position once over five runs.
  const order = scenarios.map((_, i) => scenarios[(i + r + rotate) % scenarios.length]);
  for (const s of order) {
    const res = await runScenario(s, r + 1);
    all.push(res);
    fs.writeFileSync(path.join(outDir, `run${r + 1}-${s}.json`), JSON.stringify(res, null, 2));
    console.log(
      `${s} run ${r + 1}: ${res.status} ready=${Math.round(res.service_ready_ms ?? -1)}ms cpu=${res.sample?.cpu_percent_of_machine?.toFixed(2)}% private=${Math.round((res.sample?.private_bytes_total ?? 0) / 1048576)}MiB procs=${res.sample?.process_count}`,
    );
  }
}
fs.rmSync(work, { recursive: true, force: true });

// ------------------------------------------------------------------ summary
const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s.length
    ? s.length % 2
      ? s[(s.length - 1) / 2]
      : (s[s.length / 2 - 1] + s[s.length / 2]) / 2
    : null;
};
const p95 = (xs) =>
  xs.length >= 20 ? [...xs].sort((a, b) => a - b)[Math.ceil(xs.length * 0.95) - 1] : null;
const summary = {
  label,
  package: { dir: packageDir, build_info: buildInfo, sizes },
  versions,
  settings: {
    runs,
    sample_s: sampleS,
    slow_ms: slowMs,
    diagnose,
    webview_args: webviewArgs || null,
    scenarios,
    rotated: true,
  },
  method:
    "Release package extracted from its MSI; isolated data/download/WebView2 folders; CPU = Δ(kernel+user) of owned processes / (wall × cores); private bytes summed; working set summed only as an over-count; p95 only with ≥ 20 observations.",
  scenarios: {},
};
const rows = [
  [
    "scenario",
    "run",
    "status",
    "service_ready_ms",
    "cpu_percent",
    "private_mib",
    "working_set_mib_overcounted",
    "processes",
    "desktop_private_mib",
    "webview_private_mib",
    "service_private_mib",
  ],
];
for (const s of scenarios) {
  if (s === "service_start") {
    const st = all.filter((x) => x.scenario === s && x.status === "ok");
    for (const x of all.filter((x) => x.scenario === s))
      rows.push([
        s,
        x.run,
        x.status,
        x.first_start_ms?.toFixed(0) ?? "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
      ]);
    summary.scenarios[s] = {
      runs_ok: st.length,
      median_first_start_ms: median(st.map((x) => x.first_start_ms)),
      median_second_start_ms: median(st.map((x) => x.second_start_ms)),
      first_start_ms: st.map((x) => x.first_start_ms),
      second_start_ms: st.map((x) => x.second_start_ms),
      median_private_mib: median(st.map((x) => (x.second_private_bytes ?? 0) / 1048576)),
    };
    continue;
  }
  const ok = all.filter((x) => x.scenario === s && x.status === "ok" && x.sample);
  for (const x of all.filter((x) => x.scenario === s))
    rows.push([
      s,
      x.run,
      x.status,
      x.service_ready_ms?.toFixed(0) ?? "",
      x.sample?.cpu_percent_of_machine?.toFixed(3) ?? "",
      x.sample ? (x.sample.private_bytes_total / 1048576).toFixed(1) : "",
      x.sample ? (x.sample.working_set_bytes_total_overcounted / 1048576).toFixed(1) : "",
      x.sample?.process_count ?? "",
      x.sample ? (x.sample.by_role.desktop.private_bytes / 1048576).toFixed(1) : "",
      x.sample ? (x.sample.by_role.webview.private_bytes / 1048576).toFixed(1) : "",
      x.sample ? (x.sample.by_role.service.private_bytes / 1048576).toFixed(1) : "",
    ]);
  const searches = ok.flatMap((x) => x.ui?.search_ms ?? []);
  const scrolls = ok.flatMap((x) => x.ui?.scroll_ms ?? []);
  summary.scenarios[s] = {
    runs_ok: ok.length,
    runs_not_ok: all
      .filter((x) => x.scenario === s && x.status !== "ok")
      .map((x) => x.reason ?? x.error),
    median_service_ready_ms: median(ok.map((x) => x.service_ready_ms).filter((v) => v != null)),
    median_cpu_percent: median(ok.map((x) => x.sample.cpu_percent_of_machine)),
    median_private_mib: median(ok.map((x) => x.sample.private_bytes_total / 1048576)),
    median_private_mib_by_role: Object.fromEntries(
      ["desktop", "webview", "service"].map((r) => [
        r,
        median(ok.map((x) => x.sample.by_role[r].private_bytes / 1048576)),
      ]),
    ),
    median_process_count: median(ok.map((x) => x.sample.process_count)),
    startup: {
      n: ok.length,
      median_service_ready_ms: median(ok.map((x) => x.service_ready_ms).filter((v) => v != null)),
      median_ui_first_paint_ms: median(ok.map((x) => x.ui_first_paint_ms).filter((v) => v != null)),
      median_list_ready_ms: median(ok.map((x) => x.list_ready_ms).filter((v) => v != null)),
      list_ready_upper_bounds: ok.filter((x) => x.list_ready_bound === "upper").length,
    },
    cycles: ok.filter((x) => x.cycles).map((x) => x.cycles),
    activity: ok.some((x) => x.activity)
      ? {
          runs_all_active: ok.filter(
            (x) =>
              x.activity?.every_download_gained_bytes &&
              x.activity.finished_before_window_end === 0,
          ).length,
          median_completion_ms: median(
            ok.map((x) => x.completion?.completion_ms_from_add).filter((v) => v != null),
          ),
          runs_all_sha256_match: ok.filter((x) => x.completion?.all_sha256_match).length,
          limits: ok.map((x) => x.activity?.limits),
        }
      : null,
    diag: ok.filter((x) => x.diag).map((x) => ({ run: x.run, ...x.diag })),
    ui: searches.length
      ? {
          rendered_rows: median(ok.map((x) => x.ui.rendered_rows)),
          dom_nodes: median(ok.map((x) => x.ui.dom_nodes)),
          list_first_rows_ms_median: median(
            ok.map((x) => x.list_first_rows_ms).filter((v) => v != null),
          ),
          search_ms: { n: searches.length, median: median(searches), p95: p95(searches) },
          scroll_ms: { n: scrolls.length, median: median(scrolls), p95: p95(scrolls) },
        }
      : null,
  };
}
fs.writeFileSync(path.join(outDir, "samples.csv"), rows.map((r) => r.join(",")).join("\n") + "\n");
fs.writeFileSync(path.join(outDir, "summary.json"), JSON.stringify(summary, null, 2));
console.log(`results in ${outDir}`);
