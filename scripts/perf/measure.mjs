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
//   history_1k      1,000 finished records (seeded into the database before start)
//   history_10k     10,000 finished records; also UI latencies (search, scroll) over CDP
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
const ALL = [
  "idle_visible",
  "idle_hidden",
  "ui_closed",
  "active_1",
  "active_3",
  "history_1k",
  "history_10k",
];
const scenarios = (arg("--scenarios") ?? ALL.join(",")).split(",");
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
  return { evaluate, close: () => ws.close() };
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

async function runScenario(name, runNo) {
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
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  };
  const t0 = performance.now();
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
  try {
    // The page is attached before anything is measured (first paint done).
    const page = await cdp(port);
    const firstPaint = await page.evaluate(
      `new Promise((r) => { const t0 = performance.now(); const check = () => document.querySelector("#download-list") ? r(performance.timing.domContentLoadedEventEnd - performance.timing.navigationStart) : setTimeout(check, 50); check(); })`,
    );
    result.webview_dom_ready_ms = firstPaint;
    if (name === "history_1k" || name === "history_10k") {
      const tl = performance.now();
      const shown = await page.evaluate(
        `new Promise((r) => { const t0 = performance.now(); const tick = () => document.querySelectorAll("[data-job-id]").length > 0 ? r(performance.now() - t0) : setTimeout(tick, 20); tick(); })`,
      );
      result.list_first_rows_ms = shown;
      result.list_wait_ms = performance.now() - tl;
    }
    if (name.startsWith("active_")) {
      // A local fixture in its own process (excluded from the measurement): slow, steady bodies.
      fixture = spawn(process.execPath, [path.join(import.meta.dirname, "fixture-process.mjs")], {
        stdio: ["ignore", "pipe", "ignore"],
      });
      const base = await new Promise((r) =>
        fixture.stdout.once("data", (d) => r(String(d).trim())),
      );
      const n = name === "active_1" ? 1 : 3;
      for (let k = 0; k < n; k++) dm(home, dl, ["add", `${base}/f${k}.bin?slow=25`]);
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
    const roots = name === "ui_closed" ? [servicePid(home)] : [desktop.pid, servicePid(home)];
    result.sample = await sample(roots, wv, sampleS);
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
  const order = scenarios.map((_, i) => scenarios[(i + r) % scenarios.length]);
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
  settings: { runs, sample_s: sampleS, scenarios, rotated: true },
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
