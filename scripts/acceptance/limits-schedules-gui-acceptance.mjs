#!/usr/bin/env node
// B3.1 packaged GUI acceptance of CORE-06 (speed limits) and CORE-08 (queue schedules).
//
//   node scripts/acceptance/limits-schedules-gui-acceptance.mjs --package-dir dist-packages/windows-x86_64
//
// Windows only. The NORMAL package's MSI is extracted with `msiexec /a` (nothing is installed);
// the packaged desktop starts its packaged service; the real window is driven over WebView2's
// DevTools port, so every setting is saved through the UI, the Tauri bridge and the service. The
// CLI (`dm`) and the fixture's own byte counters (a separate process: `limits-schedules-fixture.mjs`)
// are independent observers. Schedules use real near-future windows in the host's time zone;
// exhaustive calendar cases (DST, overnight, catch-up) are covered by injected-clock tests instead.
//
// Criterion: the client view within ±10 % of the limit, and the server view agreeing with it within
// a 4 MiB socket-buffer allowance over the window (both recorded).
// Throughput method (steady state): 5 s after a download receives bytes (or after a limit change),
// the fixture's bytes-written counter for that file is sampled 20 s apart (server view), with `dm get`
// byte counts at the same moments (client view; the service reports bytes as of its last poll, so
// a 20 s window keeps that lag small). Both must be within ±10 % of the configured limit.
//
// Results: acceptance-results/<label>-limits-schedules-gui/results.json and screenshots.
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { devtools } from "../lib/devtools.mjs";
import { readFirstJsonLine } from "../lib/first-line.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(arg("--package-dir", "dist-packages/windows-x86_64"));
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
const outDir = path.resolve(
  arg("--out", path.join("acceptance-results", `${buildInfo.label}-limits-schedules-gui`)),
);
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const results = {
  started_at: new Date().toISOString(),
  observer: "automated: packaged window over WebView2 DevTools, plus dm CLI and fixture counters",
  host: `${os.type()} ${os.release()} ${os.arch()}; ${os.cpus().length} logical CPUs; ${Math.round(os.totalmem() / 2 ** 30)} GiB`,
  package_dir: packageDir,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ ...detail, step, status });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 500)}`);
};
const finish = (code) => {
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.some((s) => !["passed", "note", "not run"].includes(s.status))
    ? "failed"
    : "passed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(`verdict ${results.verdict}; results in ${outDir}`);
  process.exit(code ?? (results.verdict === "passed" ? 0 : 1));
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "this script drives Windows packages only" });
  finish(2);
}
if (buildInfo.smoke_feature) throw new Error("refusing a smoke-test package: use the normal build");
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
results.package = {
  build_info: {
    source: buildInfo.source,
    built_at: buildInfo.built_at,
    signing: buildInfo.signing,
  },
  msi,
  msi_sha256: sha256(fs.readFileSync(path.join(packageDir, msi))),
};

// ------------------------------------------------------------------ extraction
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-b31gui-"));
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
const binDir = path.dirname(serviceExe);
const desktopExe = ["dm-desktop.exe", "downova.exe", "Downova.exe"]
  .map((n) => path.join(binDir, n))
  .find((p) => fs.existsSync(p));
const dmExe = path.join(binDir, "dm.exe");
const home = path.join(work, "data");
const dl = path.join(work, "downloads");
fs.mkdirSync(dl, { recursive: true });
const port = 9471;
const env = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  WEBVIEW2_USER_DATA_FOLDER: path.join(work, "webview"),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
};
results.package.extracted = {
  desktop: path.basename(desktopExe ?? "missing"),
  service: "dm-service.exe",
};

function dm(args) {
  const r = spawnSync(dmExe, ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
    timeout: 30_000,
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null, stderr: r.stderr };
  }
}
const servicePid = () => {
  try {
    return JSON.parse(fs.readFileSync(path.join(home, "run", "endpoint.json"), "utf8")).pid;
  } catch {
    return null;
  }
};
const kill = (pid, tree = false) =>
  pid && spawnSync("taskkill", ["/PID", String(pid), ...(tree ? ["/T"] : []), "/F"]);
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}
const get = (id) => dm(["get", id]).json?.data;
const add = (url, ...extra) => {
  const r = dm(["add", url, ...extra]);
  if (r.code !== 0) throw new Error(`add failed: ${JSON.stringify(r.json ?? r.stderr)}`);
  return r.json.data.job_id;
};

// ------------------------------------------------------------------ fixture (own process)
const MIB = 1024 * 1024;
const FILES = {
  lim1: [48 * MIB, 101],
  lim2: [16 * MIB, 102],
  r1: [64 * MIB, 201],
  s1: [48 * MIB, 202],
  p1: [16 * MIB, 203],
  bn1: [8 * MIB, 204],
};
const fixture = spawn(
  process.execPath,
  [
    path.join(import.meta.dirname, "limits-schedules-fixture.mjs"),
    ...Object.entries(FILES).map(([n, [size, seed]]) => `${n}:${size}:${seed}`),
  ],
  { stdio: ["ignore", "pipe", "inherit"] },
);
const { base, sha256: expected } = await readFirstJsonLine(fixture, { timeoutMs: 120_000 });
const fstats = async () => (await fetch(`${base}/__stats`)).json();
const served = async (name) => (await fstats()).files[name]?.bytes_written ?? 0;
const hashOf = (id) => {
  const j = get(id);
  return j?.phase === "completed" ? sha256(fs.readFileSync(path.join(dl, j.display_name))) : null;
};

/** Steady-state rate of one file: server and client views over 20 s after 5 s of transfer. */
async function steadyRate(name, id, limit) {
  await until(`${name} to receive bytes`, async () => (await served(name)) > 0, 60_000);
  await sleep(5000);
  const s0 = await served(name);
  const c0 = Number(get(id)?.downloaded_bytes ?? 0);
  const t0 = Date.now();
  await sleep(20_000);
  const s1 = await served(name);
  const c1 = Number(get(id)?.downloaded_bytes ?? 0);
  const secs = (Date.now() - t0) / 1000;
  const server = Math.round((s1 - s0) / secs);
  const client = Math.round((c1 - c0) / secs);
  // The limit governs what the client receives (ADR 0024's method, as in S01–S06): ±10 % on the
  // client view. The fixture's count is an independent cross-check, but with up to four range
  // connections loopback socket buffers hold megabytes, so over one window the two counts may
  // differ by the change in buffered bytes: bounded here by SOCKET_ALLOWANCE.
  const SOCKET_ALLOWANCE = 4 * 1024 * 1024;
  const difference = Math.abs(s1 - s0 - (c1 - c0));
  return {
    limit,
    server_bps: server,
    client_bps: client,
    seconds: secs,
    server_minus_client_bytes: s1 - s0 - (c1 - c0),
    socket_allowance_bytes: SOCKET_ALLOWANCE,
    within_10pct: Math.abs(client - limit) <= limit * 0.1 && difference <= SOCKET_ALLOWANCE,
  };
}

// ------------------------------------------------------------------ the window
let desktop = spawn(desktopExe, [], { env, stdio: "ignore" });
let page = null;
const shot = async (name) => {
  fs.writeFileSync(path.join(outDir, name), await page.screenshot());
  return name;
};
const ipcCalls = (cmd) =>
  page.ipc.filter((u) => u.startsWith("POST ") && u.endsWith(`/${cmd}`)).length;
/** Clicks a row of the download list by its file name (selects it). */
const selectRow = (name) =>
  page.ui(`(async () => {
    const row = await until(() => [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes(${JSON.stringify(name)})), 15000);
    row.click();
    await frame();
    return row.textContent;
  })()`);
const openOptions = () =>
  page.ui(`(async () => {
    click(byText("button", /^Options$|^অপশন/));
    await until(() => document.querySelector("[role=dialog]"), 5000);
    return true;
  })()`);
const closeDialog = () =>
  page.ui(`(async () => {
    const d = document.querySelector("[role=dialog]");
    if (!d) return true;
    click([...d.querySelectorAll("button")].find((b) => /^(Done|Close|ঠিক আছে|বন্ধ করুন)$/.test((b.textContent || b.getAttribute("aria-label") || "").trim())) ?? d.querySelector(".dialog__close"));
    await until(() => !document.querySelector("[role=dialog]"), 5000);
    return true;
  })()`);
const openManageQueues = (downloads = "Downloads", item = /^Manage queues/) =>
  page.ui(`(async () => {
    click(byText("[role=menuitem]", /^${downloads}$/));
    click(await until(() => byText("[role=menuitem]", ${item}), 5000));
    await until(() => document.querySelector("[role=dialog]"), 5000);
    return true;
  })()`);

// Near-future windows in the host's zone, on whole minutes.
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
const local = (d) => {
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat("en-GB", {
      timeZone: zone,
      weekday: "short",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    })
      .formatToParts(d)
      .map((p) => [p.type, p.value]),
  );
  return { day: parts.weekday, hhmm: `${parts.hour}:${parts.minute}` };
};
/** The next whole minute at least `aheadS` seconds from now. */
const minuteAhead = (aheadS) => {
  const t = new Date(Date.now() + aheadS * 1000);
  t.setSeconds(0, 0);
  return new Date(t.getTime() + 60_000);
};
/** Sets Night's schedule in Manage queues (window 1 only): today's day, start and end. */
const setNightSchedule = (start, end, { saveLabel = "Save schedule", day } = {}) =>
  page.ui(`(async () => {
    const item = await until(() => [...document.querySelectorAll("li")].find((li) => li.querySelector("h3")?.textContent.includes("Night")), 10000);
    const group = [...item.querySelectorAll("fieldset")].find((f) => f.querySelector("legend")?.textContent.trim() === ${JSON.stringify(saveLabel === "Save schedule" ? "Schedule" : "সময়সূচি")});
    const win = group.querySelector("ol fieldset");
    const days = [...win.querySelectorAll("label")];
    for (const l of days) {
      const box = l.querySelector("input[type=checkbox]");
      const want = l.textContent.trim() === ${JSON.stringify(day)};
      if (box && box.checked !== want) box.click();
    }
    await frame();
    const times = [...win.querySelectorAll("input[type=time]")];
    setValue(times[0], ${JSON.stringify(start)});
    setValue(times[1], ${JSON.stringify(end)});
    await frame();
    const zoneInput = [...group.querySelectorAll("input")].find((i) => i.type === "text" || i.type === "");
    const save = [...group.querySelectorAll("button")].find((b) => b.textContent.trim() === ${JSON.stringify(saveLabel)});
    // Double click: one save.
    save.click();
    save.click();
    await until(() => /Saved\\.|সংরক্ষিত হয়েছে।/.test(document.querySelector("[role=dialog]")?.textContent ?? ""), 10000);
    await sleep(500);
    const state = group.querySelector("[role=status]")?.textContent ?? null;
    return { zone: zoneInput?.value, state, checked: days.filter((l) => l.querySelector("input")?.checked).map((l) => l.textContent.trim()) };
  })()`);

try {
  await until("the service started by the desktop", () => dm(["hello"]).code === 0, 60_000);
  const hello = dm(["hello"]).json?.data;
  results.service = { backend: hello?.backend?.id, version: hello?.service_version };
  page = await devtools(port);
  await page.send("Network.enable");
  await page.ui(`until(() => document.querySelector("#download-list"), 30000).then(() => true)`);
  record(
    "G0 the packaged desktop started its packaged service",
    hello?.backend?.id === "http" ? "passed" : "failed",
    results.service,
  );

  // ---------------------------------------------------------------- CORE-06
  // G1: the overall limit through Options, double-clicked Save → one save.
  await openOptions();
  const g1 = await page.ui(`(async () => {
    const box = await until(() => labelled("Limit the overall download speed"), 10000);
    await until(() => !box.disabled, 10000);
    if (!box.checked) box.click();
    await frame();
    const value = await until(() => labelled("Overall speed"), 5000);
    setValue(value, "400");
    setValue(labelled("Unit"), "kb");
    await frame();
    const save = byText("button", /^Save speed limit$/);
    save.click();
    save.click();
    const status = await until(() => document.querySelector("[role=status][aria-label='Speed limit']")?.textContent.match(/Saved overall limit: .*/)?.[0], 10000);
    return { status };
  })()`);
  await sleep(800);
  g1.calls = ipcCalls("speed_limit_set");
  g1.service = dm(["speed-limit"]).json?.data;
  record(
    "G1 Options → overall limit 400 KB/s saved through the UI (Save double-clicked: one speed_limit_set); the service reads 400000",
    g1.calls === 1 &&
      g1.service?.global_bytes_per_second === "400000" &&
      /400 KB\/s/.test(g1.status)
      ? "passed"
      : "failed",
    g1,
  );

  // G2: invalid input is refused in the window and never sent.
  const g2 = await page.ui(`(async () => {
    const value = labelled("Overall speed");
    setValue(value, "0");
    await frame();
    const save = byText("button", /^Save speed limit$/);
    save.click();
    await sleep(400);
    const out = { aria_invalid: value.getAttribute("aria-invalid"), alert: [...document.querySelectorAll("[role=dialog] [role=alert]")].map((a) => a.textContent).join(" | "), save_disabled: save.getAttribute("aria-disabled") };
    setValue(value, "400");
    return out;
  })()`);
  g2.calls = ipcCalls("speed_limit_set");
  g2.service = dm(["speed-limit"]).json?.data?.global_bytes_per_second;
  record(
    "G2 an invalid overall speed (0) is marked invalid, explained and not sent; the service keeps 400000",
    g2.aria_invalid === "true" &&
      /1\.024 KB\/s/.test(g2.alert) &&
      g2.calls === 1 &&
      g2.service === "400000"
      ? "passed"
      : "failed",
    g2,
  );
  await shot("g1-options-overall-limit-en.png");
  await closeDialog();

  // G3: the overall limit holds on the wire.
  const lim1 = add(`${base}/lim1`);
  const g3 = await steadyRate("lim1", lim1, 400_000);
  record(
    "G3 one download under the 400 KB/s overall limit: client steady-state rate within ±10 %, the fixture's count agreeing within the socket-buffer allowance",
    g3.within_10pct ? "passed" : "failed",
    g3,
  );

  // G4: the selected download's own limit through its details (double click → one save).
  await selectRow("lim1");
  const g4 = await page.ui(`(async () => {
    const value = await until(() => labelled("Download speed"), 10000);
    await until(() => !value.disabled, 10000);
    setValue(value, "200");
    await frame();
    const set = byText("button", /^Set limit$/);
    set.click();
    set.click();
    const heading = await until(() => byText("p", /^This download's own limit: 200 KB\\/s$/)?.textContent, 10000);
    return { heading };
  })()`);
  await sleep(800);
  g4.calls = ipcCalls("download_speed_limit_set");
  g4.service = get(lim1)?.speed_limit_bytes_per_second;
  await shot("g4-details-download-limit-en.png");
  const g4rate = await steadyRate("lim1", lim1, 200_000);
  record(
    "G4 details → this download's limit 200 KB/s (Set limit double-clicked: one call); the service stores 200000; client steady state within ±10 % of 200 KB/s with the overall 400 KB/s (fixture count within the socket-buffer allowance)",
    g4.calls === 1 && g4.service === "200000" && g4rate.within_10pct ? "passed" : "failed",
    { ...g4, rate: g4rate },
  );

  // G5: switching the selection shows each download's authoritative value.
  const lim2 = add(`${base}/lim2`);
  await selectRow("lim2");
  const g5b = await page.ui(`(async () => {
    await until(() => byText("p", /^No limit of its own/), 10000);
    return { field: labelled("Download speed").value };
  })()`);
  await selectRow("lim1");
  const g5a = await page.ui(`(async () => {
    await until(() => byText("p", /^This download's own limit: 200 KB\\/s$/), 10000);
    return { field: labelled("Download speed").value };
  })()`);
  record(
    "G5 switching the selected download shows that download's own value (lim2: none and an empty field; lim1: 200 KB/s)",
    g5b.field === "" && g5a.field === "200" ? "passed" : "failed",
    { lim2: g5b, lim1: g5a },
  );

  // G6: a service restart (a new service instance): the window reattaches and reads both values.
  const oldPid = servicePid();
  dm(["shutdown"]);
  await until("the service to stop", () => dm(["hello"]).code !== 0, 30_000);
  spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
    detached: true,
  }).unref();
  await until(
    "the restarted service",
    () => dm(["hello"]).code === 0 && servicePid() !== oldPid,
    30_000,
  );
  const g6 = await page.ui(`(async () => {
    await until(() => [...document.querySelectorAll("tbody tr")].some((r) => r.textContent.includes("lim1")), 30000);
    await sleep(1500);
    const row = [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes("lim1"));
    row.click();
    const heading = await until(() => byText("p", /^This download's own limit: /)?.textContent, 15000);
    return { heading };
  })()`);
  await openOptions();
  const g6opt = await page.ui(`(async () => {
    const value = await until(() => labelled("Overall speed"), 10000);
    await until(() => value.value !== "" && !value.disabled, 15000);
    return { box: labelled("Limit the overall download speed").checked, value: value.value, unit: labelled("Unit").value };
  })()`);
  await closeDialog();
  record(
    "G6 after a service restart the window reattaches and reads both saved values (overall 400 KB/s in Options; lim1's own 200 KB/s in its details)",
    /200 KB\/s/.test(g6.heading) && g6opt.box && g6opt.value === "400" && g6opt.unit === "kb"
      ? "passed"
      : "failed",
    { details: g6, options: g6opt },
  );
  dm(["cancel", lim1]);
  dm(["cancel", lim2]);

  // ---------------------------------------------------------------- CORE-08
  // G7: a queue and its schedule through Manage queues (a near-future window, today only).
  const startAt = minuteAhead(75);
  const endAt = new Date(startAt.getTime() + 2 * 60_000);
  const win = { day: local(startAt).day, start: local(startAt).hhmm, end: local(endAt).hhmm };
  await openManageQueues();
  const created = await page.ui(`(async () => {
    setValue(labelled("New queue name"), "Night");
    click(byText("button", /^Add queue$/));
    await until(() => [...document.querySelectorAll("li h3")].some((h) => h.textContent.includes("Night")), 10000);
    return true;
  })()`);
  const before = ipcCalls("queues_set_schedule");
  const g7 = await setNightSchedule(win.start, win.end, { day: win.day });
  g7.calls = ipcCalls("queues_set_schedule") - before;
  const night = (dm(["queues"]).json?.data?.queues ?? []).find((q) => q.name === "Night");
  g7.service = { schedule: night?.schedule, state: night?.schedule_state };
  await shot("g7-manage-queues-schedule-en.png");
  await closeDialog();
  record(
    `G7 Manage queues → Night: schedule ${win.day} ${win.start}–${win.end} in the preselected host zone (${zone}), saved once on a double click; the service stores it and reports the next window`,
    created === true &&
      g7.zone === zone &&
      g7.calls === 1 &&
      night?.schedule?.timezone === zone &&
      night.schedule.windows?.[0]?.start === win.start &&
      night.schedule_state?.in_window === false &&
      Date.parse(night.schedule_state?.next_change_at) === startAt.getTime()
      ? "passed"
      : "failed",
    { window: win, ...g7 },
  );

  // G8: waiting reason and next-window text for downloads of that queue; a manual pause.
  const r1 = add(`${base}/r1`, "--queue", night.queue_id);
  const s1 = add(`${base}/s1?norange=1`, "--queue", night.queue_id);
  const p1 = add(`${base}/p1`, "--queue", night.queue_id);
  await selectRow("p1");
  await page.ui(
    `(async () => { click(byText("[role=toolbar] button", /^Stop$/)); await sleep(800); return true; })()`,
  );
  await selectRow("r1");
  const nextText = `Next window: ${local(startAt).day} ${win.start} (${zone}).`;
  const g8 = await page.ui(`(async () => {
    await until(() => byText("h2", /^Waiting for the schedule of Night$/), 10000);
    const row = [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes("r1"));
    return { row: row?.textContent ?? null, note: byText("section", /Waiting for the schedule of Night/)?.textContent ?? null };
  })()`);
  g8.bytes = { r1: await served("r1"), s1_plain: await served("s1") };
  g8.cli = { r1: get(r1)?.blockers, p1: get(p1)?.desired_state };
  await shot("g8-waiting-for-schedule-en.png");
  record(
    "G8 before the window: r1's row says “Waiting: queue schedule” (not the slot limit) and its details name Night's next window in the queue's zone; no bytes served; p1 paused by the user",
    /Waiting: queue schedule/.test(g8.row ?? "") &&
      !/all download slots in use/.test(g8.row ?? "") &&
      (g8.note ?? "").includes(nextText) &&
      g8.bytes.r1 === 0 &&
      g8.bytes.s1_plain === 0 &&
      g8.cli.p1 === "paused"
      ? "passed"
      : "failed",
    { expected_note: nextText, ...g8 },
  );

  // G9: the window opens: both start; the paused one stays paused.
  await until("the window to open", () => Date.now() >= startAt.getTime() + 2000, 200_000);
  await until(
    "r1 and s1 to receive bytes",
    async () => (await served("r1")) > 0 && (await served("s1")) > 0,
    60_000,
  );
  await sleep(3000);
  const g9 = { r1: get(r1), s1: get(s1), p1: get(p1), p1_served: await served("p1") };
  await shot("g9-window-open-en.png");
  record(
    "G9 when the window opens r1 and s1 start (fixture bytes > 0, no schedule blocker); p1 stays paused with 0 bytes served",
    !g9.r1.blockers.includes("schedule_window") &&
      !g9.s1.blockers.includes("schedule_window") &&
      g9.p1.desired_state === "paused" &&
      g9.p1_served === 0
      ? "passed"
      : "failed",
    {
      r1: { blockers: g9.r1.blockers, bytes: g9.r1.downloaded_bytes },
      s1: { blockers: g9.s1.blockers, bytes: g9.s1.downloaded_bytes },
      p1: { state: g9.p1.desired_state, served: g9.p1_served },
    },
  );

  // G10: the window closes: the checkpointed r1 is held; s1 (no range support) finishes, explained.
  await until("the window to close", () => Date.now() >= endAt.getTime() + 3000, 200_000);
  await sleep(3000);
  const r1a = await served("r1");
  const s1a = await served("s1");
  await sleep(5000);
  const r1b = await served("r1");
  const s1b = await served("s1");
  await selectRow("s1");
  const g10 = await page.ui(`(async () => {
    const title = await until(() => byText("h2", /^Finishing outside the schedule window$/), 15000);
    return { note: title.closest("section")?.textContent ?? null };
  })()`);
  g10.r1 = get(r1);
  g10.s1 = get(s1);
  await shot("g10-finishing-outside-window-en.png");
  record(
    "G10 at the window's end r1 (resumable, checkpointed) is held: schedule_window, no further bytes; s1 (server without ranges, no checkpoint) keeps running and its details explain why",
    g10.r1.blockers.includes("schedule_window") &&
      r1b === r1a &&
      s1b > s1a &&
      g10.s1.schedule_exemption === "no_resumable_checkpoint" &&
      /would lose its progress/.test(g10.note ?? "")
      ? "passed"
      : "failed",
    {
      r1: { blockers: g10.r1.blockers, served_5s: [r1a, r1b] },
      s1: { exemption: g10.s1.schedule_exemption, served_5s: [s1a, s1b] },
      note: g10.note,
    },
  );
  await until("s1 to complete", () => get(s1)?.phase === "completed", 300_000);
  const g10b = await page.ui(
    `(async () => { await sleep(1500); return { still: !!byText("h2", /^Finishing outside the schedule window$/) }; })()`,
  );
  record(
    "G10b s1 completes byte-exact outside the window and the explanation is gone",
    hashOf(s1) === expected.s1 && !g10b.still && !get(s1).schedule_exemption ? "passed" : "failed",
    { sha256_ok: hashOf(s1) === expected.s1, note_still_shown: g10b.still },
  );

  // G11: editing the window to include now resumes r1 from its checkpoint; p1 stays paused.
  const now = new Date();
  const editStart = local(new Date(now.getTime() - 60_000)).hhmm;
  const editEnd = local(new Date(now.getTime() + 40 * 60_000)).hhmm;
  const r1Before = { ...(await fstats()).files.r1 };
  await openManageQueues();
  const g11 = await setNightSchedule(editStart, editEnd, {
    day: local(new Date(now.getTime() - 60_000)).day,
  });
  await closeDialog();
  await until("r1 to complete", () => get(r1)?.phase === "completed", 400_000);
  const r1stats = (await fstats()).files.r1;
  record(
    "G11 editing Night's window to include now admits r1 again; it resumes with new range requests only (no full restart) and completes byte-exact; p1 stays paused",
    hashOf(r1) === expected.r1 &&
      r1stats.full_responses === r1Before.full_responses &&
      r1stats.range_responses > r1Before.range_responses &&
      get(p1).desired_state === "paused" &&
      (await served("p1")) === 0
      ? "passed"
      : "failed",
    {
      edit: { start: editStart, end: editEnd, state: g11.state },
      r1_before_edit: { full: r1Before.full_responses, ranges: r1Before.range_responses },
      r1_after: {
        full: r1stats.full_responses,
        ranges: r1stats.range_responses,
        range_starts: r1stats.range_starts.slice(0, 16),
      },
    },
  );

  // G12: restart: limits, schedule and the manual pause persist.
  const pidBefore = servicePid();
  dm(["shutdown"]);
  await until("the service to stop", () => dm(["hello"]).code !== 0, 30_000);
  spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
    detached: true,
  }).unref();
  await until(
    "the restarted service",
    () => dm(["hello"]).code === 0 && servicePid() !== pidBefore,
    30_000,
  );
  await sleep(3000);
  await openManageQueues();
  const g12 = await page.ui(`(async () => {
    const item = await until(() => [...document.querySelectorAll("li")].find((li) => li.querySelector("h3")?.textContent.includes("Night")), 15000);
    const group = [...item.querySelectorAll("fieldset")].find((f) => f.querySelector("legend")?.textContent.trim() === "Schedule");
    const times = [...group.querySelectorAll("ol fieldset input[type=time]")].map((i) => i.value);
    return { times, state: group.querySelector("[role=status]")?.textContent ?? null };
  })()`);
  await closeDialog();
  const nightAfter = (dm(["queues"]).json?.data?.queues ?? []).find((q) => q.name === "Night");
  record(
    "G12 after a restart: Night's schedule (shown in Manage queues and by dm queues), the overall limit and p1's manual pause persist",
    g12.times[0] === editStart &&
      g12.times[1] === editEnd &&
      nightAfter?.schedule?.windows?.[0]?.start === editStart &&
      dm(["speed-limit"]).json?.data?.global_bytes_per_second === "400000" &&
      get(p1).desired_state === "paused"
      ? "passed"
      : "failed",
    { manage_queues: g12, service_schedule: nightAfter?.schedule },
  );

  // G13: Bangla — the window explains the waiting reason and next window; Options in Bangla.
  await openOptions();
  await page.ui(`(async () => {
    const lang = labelled("Language");
    setValue(lang, "bn");
    await until(() => labelled("ভাষা"), 5000);
    return true;
  })()`);
  const bnOptions = await page.ui(`(async () => {
    const value = await until(() => labelled("মোট গতি"), 10000);
    return { value: value.value, heading: byText("h3", /^গতির সীমা$/)?.textContent ?? null };
  })()`);
  await shot("g13-options-overall-limit-bn.png");
  await closeDialog();
  const bnStart = minuteAhead(600);
  const bnEnd = new Date(bnStart.getTime() + 5 * 60_000);
  await openManageQueues("ডাউনলোড", /^কিউ পরিচালনা/);
  const bnLocalDays = {
    Mon: "সোম",
    Tue: "মঙ্গল",
    Wed: "বুধ",
    Thu: "বৃহঃ",
    Fri: "শুক্র",
    Sat: "শনি",
    Sun: "রবি",
  };
  const bnSaved = await setNightSchedule(local(bnStart).hhmm, local(bnEnd).hhmm, {
    saveLabel: "সময়সূচি সংরক্ষণ করুন",
    day: bnLocalDays[local(bnStart).day],
  });
  await shot("g13-manage-queues-schedule-bn.png");
  await closeDialog();
  const bn1 = add(`${base}/bn1`, "--queue", nightAfter.queue_id);
  await selectRow("bn1");
  const g13 = await page.ui(`(async () => {
    await until(() => byText("h2", /^Night-এর সময়সূচির অপেক্ষায়$/), 15000);
    const row = [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes("bn1"));
    return { row: row?.textContent ?? null, note: byText("section", /Night-এর সময়সূচির অপেক্ষায়/)?.textContent ?? null };
  })()`);
  await shot("g13-waiting-for-schedule-bn.png");
  record(
    "G13 Bangla: Options shows the overall limit (মোট গতি = 400), Night's schedule is saved from the Bangla Manage queues, and a waiting download says “অপেক্ষা: কিউয়ের সময়সূচি” with the next window in the queue's zone",
    bnOptions.value === "400" &&
      /সংরক্ষিত হয়েছে|পরের সময়সীমার অপেক্ষায়/.test(JSON.stringify(bnSaved)) &&
      /অপেক্ষা: কিউয়ের সময়সূচি/.test(g13.row ?? "") &&
      /পরের সময়সীমা: .*\(/.test(g13.note ?? "") &&
      (g13.note ?? "").includes(zone) &&
      (await served("bn1")) === 0 &&
      get(bn1)?.blockers?.includes("schedule_window")
      ? "passed"
      : "failed",
    { options: bnOptions, schedule: bnSaved, ...g13 },
  );
  // Back to English for any later run in this profile.
  await openOptions();
  await page.ui(
    `(async () => { setValue(labelled("ভাষা"), "en"); await until(() => labelled("Language"), 5000); return true; })()`,
  );
  await closeDialog();
  results.fixture = await fstats();
} catch (e) {
  record("run", "failed", { error: String(e?.stack ?? e) });
  try {
    await shot("failure.png");
  } catch {
    /* no page */
  }
} finally {
  page?.close();
  kill(desktop.pid, true);
  kill(servicePid(), true);
  fixture.kill();
  await sleep(1500);
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    /* a just-ended process may still hold a file; the folder is in TEMP */
  }
}
record("installed app", "not run", {
  reason:
    "nothing is installed by this script (msiexec /a); installed-app acceptance is a human check",
});
finish();
