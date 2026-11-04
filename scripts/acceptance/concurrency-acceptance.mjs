#!/usr/bin/env node
// Packaged acceptance for configurable concurrency (CORE-02, CORE-05, ADR 0023).
//
//   node scripts/acceptance/concurrency-acceptance.mjs --package-dir dist-packages/windows-x86_64
//        [--out acceptance-results/concurrency-<label>]
//
// Windows only for now (MSI extraction and process control); other OSes report "not run".
//
// Runs the NORMAL release package (a smoke-test package is refused), extracted from its MSI with
// `msiexec /a`: nothing is installed, and the data, download and WebView2 folders are this run's
// own (DOWNOVA_HOME, DOWNOVA_DOWNLOAD_DIR, WEBVIEW2_USER_DATA_FOLDER). If another Downova desktop
// is running the run stops; only processes this script started are ever ended.
//
// What is driven, and through which path:
// - the packaged desktop window, through its WebView's DevTools port (real Tauri commands and
//   capability grants): Options → Simultaneous downloads (keyboard entry and Enter, a double
//   submission, an invalid value, Bangla), and Downloads → Manage queues… → Main limit;
// - the packaged service through the packaged `dm` CLI: adding, pausing, queues, the limit while
//   transfers run, and every state read;
// - a controlled fixture (range-capable, strong ETag, paced bodies) in its own process
//   (concurrency-fixture.mjs), which counts
//   concurrent requests and bytes served per file.
//
// Rows record exactly what was observed. A human check (screen reader, OS zoom, the look of the
// window) is never inferred from these rows.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { payload, sha256 } from "../lib/fixture.mjs";
import { readFirstLine } from "../lib/first-line.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(arg("--package-dir", "dist-packages/windows-x86_64"));
const outDir = path.resolve(
  arg("--out", path.join("acceptance-results", `concurrency-${path.basename(packageDir)}`)),
);
fs.mkdirSync(outDir, { recursive: true });
const results = { started_at: new Date().toISOString(), package_dir: packageDir, steps: [] };
const record = (step, status, detail = {}) => {
  // The step's own fields come last, so no detail can overwrite them (B2.32: a detail called
  // `status` replaced U1's "passed"/"failed" with the window's status text, which the verdict ignored).
  results.steps.push({ ...detail, step, status });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 400)}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const finish = (code) => {
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.some((s) => s.status !== "passed" && s.status !== "note")
    ? "failed"
    : "passed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2));
  console.log(`verdict ${results.verdict}; results in ${outDir}`);
  process.exit(code ?? (results.verdict === "passed" ? 0 : 1));
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "this script drives Windows packages only" });
  finish(0);
}

// ------------------------------------------------------------------ package
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
if (buildInfo.smoke_feature) throw new Error("refusing a smoke-test package: use the normal build");
results.build_info = { source: buildInfo.source, built_at: buildInfo.built_at };
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-conc-"));
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
results.msi_sha256 = sha256(fs.readFileSync(path.join(packageDir, msi)));
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
const wv = path.join(work, "webview");
fs.mkdirSync(dl, { recursive: true });
const port = 9470;
const env = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  WEBVIEW2_USER_DATA_FOLDER: wv,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  // The service's diagnostic trace of committed checkpoints (also in production builds).
  DOWNOVA_CHECKPOINT_TRACE: path.join(work, "trace.jsonl"),
  // The engine's own permit trace (B2.32): every per-host permit taken and released, with the count
  // held after it. Kept in the results folder as evidence. This is the exact budget; the client TCP
  // samples (300 ms) and the fixture's socket count are outside views of it.
  DOWNOVA_PERMIT_TRACE: path.join(outDir, "permit-trace.jsonl"),
};
fs.rmSync(env.DOWNOVA_PERMIT_TRACE, { force: true });
/** { events, acquires, max_held, at_max, hosts } from the permit trace (one service process at a time). */
const permits = () => {
  const lines = fs.existsSync(env.DOWNOVA_PERMIT_TRACE)
    ? fs
        .readFileSync(env.DOWNOVA_PERMIT_TRACE, "utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];
  const top = lines.reduce((m, l) => (l.held > (m?.held ?? -1) ? l : m), null);
  return {
    events: lines.length,
    acquires: lines.filter((l) => l.event === "acquire").length,
    max_held: top?.held ?? null,
    at_max: top ? new Date(top.t_ms).toISOString() : null,
    hosts: new Set(lines.map((l) => l.host_hash)).size,
  };
};
const traced = (id) =>
  (fs.existsSync(env.DOWNOVA_CHECKPOINT_TRACE)
    ? fs.readFileSync(env.DOWNOVA_CHECKPOINT_TRACE, "utf8").split("\n").filter(Boolean)
    : []
  )
    .map((l) => JSON.parse(l))
    .some((e) => e.event === "checkpoint" && e.job_id === id && e.confirmed > 0);
const ps = (cmd) =>
  spawnSync("powershell", ["-NoProfile", "-Command", cmd], { encoding: "utf8" }).stdout;
const otherDesktop = ps(
  "Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(dm-desktop|downova)\\.exe$' } | ForEach-Object { $_.CommandLine }",
)
  .split("\n")
  .some((l) => l.trim() && !l.toLowerCase().includes(work.toLowerCase()));
if (otherDesktop) {
  record("isolation", "not run", { reason: "another Downova desktop is running" });
  finish(2);
}

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
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}`);
}
const jobs = () => dm(["list"]).json?.data?.items ?? [];
const job = (id) => jobs().find((j) => j.job_id === id);
const bytes = (j) => Number(j?.downloaded_bytes ?? 0);

// ------------------------------------------------------------------ fixture
const SIZE = 32 * 1024 * 1024;
// c1–c6 the six, c7 a late Main download, c8 one in a stopped queue, c9 cancelled while running.
const NAMES = Array.from({ length: 9 }, (_, i) => `c${i + 1}.bin`);
const seeds = NAMES.map((n, i) => [n, 1000 + i]);
// Only the expected hashes are kept: holding the payloads twice (here and in the fixture process)
// starved this 7.5 GB machine and slowed the fixture (B2.29 acceptance run 4).
const expected = Object.fromEntries(seeds.map(([n, s]) => [n, sha256(payload(SIZE, s))]));
// The fixture runs in its own process: this script's blocking CLI calls must not delay its
// socket bookkeeping (an earlier in-process counter over-reported overlap for that reason).
const fixture = spawn(
  process.execPath,
  [
    path.join(import.meta.dirname, "concurrency-fixture.mjs"),
    String(SIZE),
    ...seeds.map(([n, s]) => `${n}:${s}`),
  ],
  { stdio: ["ignore", "pipe", "inherit"] },
);
// Chunk-safe and bounded (B2.33, scripts/lib/first-line.mjs).
const base = (await readFirstLine(fixture, { timeoutMs: 120_000 })).trim();
/**
 * The client side of the host budget (B2.31): the packaged service's ESTABLISHED TCP connections to
 * the fixture, sampled from the OS every 300 ms. A connection the client closed leaves ESTABLISHED at
 * once, so unlike the fixture's own count this does not depend on when the server notices a close
 * (one run counted 9 server-side while F1 of the same run counted 8). Sampling can miss a sub-sample
 * handover; it does catch a sustained excess.
 */
const fixturePort = new URL(base).port;
const tcp = { samples: 0, max: 0, at_max: null };
const sampleTcp = () => {
  const pid = String(servicePid() ?? "");
  if (!pid) return;
  const out = spawnSync("netstat", ["-ano", "-p", "TCP"], { encoding: "utf8" }).stdout ?? "";
  const n = out
    .split("\n")
    .map((l) => l.trim().split(/\s+/))
    .filter(
      (c) => c[2] === `127.0.0.1:${fixturePort}` && c[3] === "ESTABLISHED" && c[4] === pid,
    ).length;
  tcp.samples += 1;
  if (n > tcp.max) {
    tcp.max = n;
    tcp.at_max = new Date().toISOString();
  }
};
let tcpTimer = null;
/** { served: {name: {requests, bytes}}, overlap, raw_peak } from the fixture process. */
const stats = async () => (await fetch(`${base}/__stats`)).json();
// 30 ms per 16 KiB per request (the fixture's step): about 0.5 MB/s per request, so eight requests share about 4 MB/s
// and six 32 MiB files take about a minute — long enough to change limits while they run, with a
// 1 MiB rotation turn of about 2 s.
const SLOW = 30;
const url = (n) => `${base}/${n}?slow=${SLOW}`;

// ------------------------------------------------------------------ desktop + DevTools
let desktop = spawn(desktopExe, [], { env, stdio: "ignore" });
let page = null;
try {
  await until("the service started by the desktop", () => dm(["hello"]).code === 0, 60_000);
  const hello = dm(["hello"]).json?.data;
  record(
    "D0 the packaged desktop started its packaged service (native HTTP backend)",
    hello?.backend?.id === "http" ? "passed" : "failed",
    {
      backend: hello?.backend?.id,
      service_version: hello?.service_version,
    },
  );
  page = await devtools(port);
  await page.ui(`until(() => document.querySelector("#download-list"), 30000).then(() => true)`);

  // U1: the global limit through the real Options dialog, by keyboard.
  const optionsOpen = await page.ui(`(async () => {
    const options = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Options");
    options.click();
    const input = await until(() => labelled("Maximum active downloads"), 10000);
    await until(() => !input.disabled && input.value !== "", 10000);
    return { initial: input.value, label: !!input, describedBy: input.getAttribute("aria-describedby") };
  })()`);
  // Count the page's real IPC requests (Tauri's custom-protocol requests) from here on.
  await page.send("Network.enable");
  page.ipc.length = 0;
  await page.ui(`(() => {
    labelled("Maximum active downloads").focus();
    labelled("Maximum active downloads").select();
    return true;
  })()`);
  await page.send("Input.insertText", { text: "6" });
  // Enter twice in a row: one save.
  for (let k = 0; k < 2; k++) {
    await page.send("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: "Enter",
      code: "Enter",
      windowsVirtualKeyCode: 13,
      text: "\r",
    });
    await page.send("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: "Enter",
      code: "Enter",
      windowsVirtualKeyCode: 13,
    });
  }
  const saved = await page.ui(`(async () => {
    const status = await until(() => [...document.querySelectorAll("[role=status]")].find((s) => /Saved global limit: 6/.test(s.textContent)), 10000);
    await sleep(1500);
    return { status: status?.textContent ?? null };
  })()`);
  // Only POSTs are calls; a CORS preflight (OPTIONS) to the IPC origin is not a second save.
  const setCalls = () =>
    page.ipc.filter((u) => u.startsWith("POST ") && u.includes("concurrency_set")).length;
  saved.calls = setCalls();
  saved.ipc_seen = [...page.ipc];
  const cliAfterUi = dm(["concurrency"]).json?.data?.max_active_downloads;
  record(
    "U1 Options → Simultaneous downloads: 6 entered by keyboard and Enter pressed twice; one concurrency_set call; the service reports 6",
    optionsOpen?.initial === "3" && saved?.calls === 1 && cliAfterUi === 6 ? "passed" : "failed",
    {
      initial: optionsOpen?.initial,
      set_calls: saved?.calls,
      ipc_requests_seen: saved?.ipc_seen,
      ui_status_text: saved?.status,
      service_limit: cliAfterUi,
    },
  );

  // U2: an invalid value is refused in the window and never sent.
  const invalid = await page.ui(`(async () => {
    const input = labelled("Maximum active downloads");
    setValue(input, "17");
    await frame();
    const save = [...document.querySelectorAll("button")].find((b) => /^Save download limit/.test(b.textContent.trim()));
    save.click();
    await sleep(500);
    const alert = [...document.querySelectorAll("[role=alert]")].map((a) => a.textContent).join(" | ");
    const out = { aria_invalid: input.getAttribute("aria-invalid"), alert, save_disabled: save.getAttribute("aria-disabled") };
    setValue(input, "6");
    return out;
  })()`);
  if (invalid) invalid.calls = setCalls();
  record(
    "U2 an invalid limit (17) is marked invalid, explained, and not sent",
    invalid?.aria_invalid === "true" &&
      /1 to 16/.test(invalid?.alert ?? "") &&
      invalid?.calls === 1 &&
      dm(["concurrency"]).json?.data?.max_active_downloads === 6
      ? "passed"
      : "failed",
    invalid ?? {},
  );

  // U3: Bangla labels and the both-limits explanation, then back to English.
  const bangla = await page.ui(`(async () => {
    const lang = labelled("Language");
    const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set;
    set.call(lang, "bn");
    lang.dispatchEvent(new Event("change", { bubbles: true }));
    const input = await until(() => labelled("সর্বোচ্চ সক্রিয় ডাউনলোড"), 5000);
    const section = input.closest("section");
    const text = section?.textContent ?? "";
    const out = {
      heading: section?.querySelector("h3")?.textContent ?? null,
      both_limits: text.includes("দুটোই ৬ করুন"),
      menu_named_in_bangla: text.includes("ডাউনলোড → কিউ পরিচালনা"),
      value: input.value,
    };
    const lang2 = labelled("ভাষা");
    set.call(lang2, "en");
    lang2.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => labelled("Maximum active downloads"), 5000);
    return out;
  })()`);
  record(
    "U3 Bangla: the section, label and both-limits explanation are Bangla (menu named as shown in Bangla)",
    bangla?.heading === "একসাথে ডাউনলোড" &&
      bangla.both_limits &&
      bangla.menu_named_in_bangla &&
      bangla.value === "6"
      ? "passed"
      : "failed",
    bangla ?? {},
  );

  // S1: saving unrelated settings keeps the global limit (download settings in the window,
  // history retention and notifications through the CLI).
  const unrelated = await page
    .ui(
      `(async () => {
    const collision = labelled("When a file with the same name exists");
    setValue(collision, "ask");
    const save = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Save download settings");
    save.click();
    await until(() => /Download settings saved\\./.test(document.querySelector("[role=dialog]")?.textContent ?? ""), 10000);
    return { collision: collision.value };
  })()`,
    )
    .catch((e) => ({ error: String(e) }));
  const historySet = dm(["history-set", "30"]);
  const notificationsSet = dm(["notifications-set", "on"]);
  const afterUnrelated = dm(["concurrency"]).json?.data?.max_active_downloads;
  record(
    "S1 saving unrelated settings (collision policy in Options, history retention and notifications by CLI) keeps the global limit at 6",
    unrelated?.collision === "ask" && historySet.code === 0 && afterUnrelated === 6
      ? "passed"
      : "failed",
    {
      window: unrelated,
      history_set_exit: historySet.code,
      notifications_set_exit: notificationsSet.code,
      notifications_detail: notificationsSet.json?.data ?? notificationsSet.json?.error ?? null,
      limit_after: afterUnrelated,
    },
  );

  // V1: invalid values through the CLI are refused; the effective value does not change.
  const zero = dm(["concurrency-set", "0"]);
  const tooMany = dm(["concurrency-set", "17"]);
  const afterInvalid = dm(["concurrency"]).json?.data?.max_active_downloads;
  record(
    "V1 dm concurrency-set 0 and 17 are refused (non-zero exit) and the effective limit stays 6",
    zero.code !== 0 && tooMany.code !== 0 && afterInvalid === 6 ? "passed" : "failed",
    { zero_exit: zero.code, seventeen_exit: tooMany.code, limit_after: afterInvalid },
  );

  // U4 (B2.31): Options explains what the saved limits allow. With global 6 and Main 3 it says Main
  // can run 3 and offers "Change queue limit"; that button opens Manage queues, Main is set to 6
  // there, and closing it returns to Options, which then says 6. Screenshots in en and bn.
  const explanation = `(() => {
    const group = [...document.querySelectorAll("[role=group]")].find((g) => /What this allows|এতে যা চলতে পারে/.test(g.textContent));
    if (!group) return null;
    const value = (term) => [...group.querySelectorAll("dt")].find((d) => d.textContent.trim() === term)?.nextElementSibling?.textContent ?? null;
    return {
      global: value("Saved limit for all queues"),
      main: value("Main queue limit"),
      can_run: value("Main can run at once"),
      active: value("Active now"),
      queue_restricts: /own limit \\(\\d+\\) is lower/.test(group.textContent),
      change_button: [...group.querySelectorAll("button")].some((b) => b.textContent.trim() === "Change queue limit"),
    };
  })()`;
  const shot = async (name) => {
    // Bring the explanation into view (it sits below the fold in the Options dialog).
    await page.ui(
      `(() => { const g = [...document.querySelectorAll("[role=group]")].find((x) => /What this allows|এতে যা চলতে পারে/.test(x.textContent)); g?.scrollIntoView({ block: "center" }); return !!g; })()`,
    );
    await sleep(300);
    const png = await page.send("Page.captureScreenshot", { format: "png" });
    fs.writeFileSync(path.join(outDir, name), Buffer.from(png.data, "base64"));
    return name;
  };
  const limitsBefore = await page.ui(explanation).catch((e) => ({ error: String(e) }));
  const shotBefore = await shot("options-limits-global6-main3-en.png");
  const queueSet = await page
    .ui(
      `(async () => {
    const group = [...document.querySelectorAll("[role=group]")].find((g) => /What this allows/.test(g.textContent));
    [...group.querySelectorAll("button")].find((b) => b.textContent.trim() === "Change queue limit").click();
    const input = await until(() => labelled("Downloads at once"), 5000);
    setValue(input, "6");
    const item = input.closest("li");
    [...item.querySelectorAll("button")].find((b) => b.textContent.trim() === "Set limit").click();
    await until(() => /Saved\\./.test(document.querySelector("[role=dialog]")?.textContent ?? ""), 10000);
    [...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.trim() === "Done").click();
    // Back in Options, which reads the queue limits again.
    await until(() => [...document.querySelectorAll("[role=dialog] h2, [role=dialog] h1")].some((h) => h.textContent.trim() === "Options"), 5000);
    await until(() => [...document.querySelectorAll("dt")].some((d) => d.textContent.trim() === "Main queue limit" && d.nextElementSibling?.textContent === "6"), 10000);
    return true;
  })()`,
    )
    .catch((e) => String(e));
  const limitsAfter = await page.ui(explanation).catch((e) => ({ error: String(e) }));
  const shotAfter = await shot("options-limits-global6-main6-en.png");
  // The language select is switched as in U3 (its own value setter, then change).
  const banglaSwitch = await page
    .ui(
      `(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set;
    const lang = labelled("Language");
    set.call(lang, "bn");
    lang.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => [...document.querySelectorAll("[role=group]")].some((g) => /এতে যা চলতে পারে/.test(g.textContent)), 5000);
    return true;
  })()`,
    )
    .catch((e) => String(e));
  const shotBangla = await shot("options-limits-global6-main6-bn.png");
  await page
    .ui(
      `(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set;
    const lang = labelled("ভাষা");
    set.call(lang, "en");
    lang.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => labelled("Maximum active downloads"), 5000);
    [...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.trim() === "Close")?.click();
    return true;
  })()`,
    )
    .catch((e) => String(e));
  const mainQueue = (dm(["queues"]).json?.data?.queues ?? []).find((q) => q.queue_id === "main");
  record(
    'U4 Options explains the limits: global 6 with Main 3 → Main can run 3 and "Change queue limit"; that opens Manage queues, Main set to 6 there, back in Options → Main can run 6 (service: Main 6, global still 6)',
    limitsBefore?.global === "6" &&
      limitsBefore.main === "3" &&
      limitsBefore.can_run === "3" &&
      limitsBefore.queue_restricts &&
      limitsBefore.change_button &&
      banglaSwitch === true &&
      queueSet === true &&
      limitsAfter?.main === "6" &&
      limitsAfter.can_run === "6" &&
      !limitsAfter.change_button &&
      mainQueue?.max_active === 6 &&
      dm(["concurrency"]).json?.data?.max_active_downloads === 6
      ? "passed"
      : "failed",
    {
      bangla: banglaSwitch,
      limitsBefore,
      limitsAfter,
      ui: queueSet,
      main: mainQueue,
      screenshots: [shotBefore, shotAfter, shotBangla],
    },
  );

  // T1: six large range-capable files from one host, all receiving bytes before any finishes.
  const ids = [];
  for (const n of NAMES.slice(0, 6)) ids.push(dm(["add", url(n)]).json?.data?.job_id);
  tcpTimer = setInterval(sampleTcp, 300);
  const firstByte = {};
  const t0 = Date.now();
  let completedEarly = false;
  const timeline = [];
  await until(
    "six files to receive data",
    () => {
      const now = jobs();
      for (const id of ids) {
        const j = now.find((x) => x.job_id === id);
        if (j && bytes(j) > 0 && firstByte[id] === undefined) firstByte[id] = Date.now() - t0;
        if (j?.phase === "completed") completedEarly = true;
      }
      const second = Math.floor((Date.now() - t0) / 1000);
      if (timeline.at(-1)?.s !== second)
        timeline.push({
          s: second,
          jobs: ids.map((id) => {
            const j = now.find((x) => x.job_id === id);
            return `${j?.phase ?? "?"}:${(j?.blockers ?? []).join("+")}:${bytes(j)}`;
          }),
        });
      return Object.keys(firstByte).length === 6;
    },
    90_000,
  ).catch(() => null);
  const t1Stats = await stats();
  const t1Permits = permits();
  const servedSnapshot = Object.fromEntries(NAMES.slice(0, 6).map((n) => [n, t1Stats.served[n]]));
  const windowRows = await page.ui(`(() => {
    const rows = [...document.querySelectorAll("tbody tr[data-row]")];
    return rows.map((r) => r.textContent.replace(/\\s+/g, " ").trim()).filter((t) => /\\d+(\\.\\d+)?\\s?%/.test(t)).length;
  })()`);
  record(
    "T1 six 32 MiB files from one host: all six receive real bytes (service and fixture) before any completes; the window shows progress on six rows; host requests stay ≤ 8",
    Object.keys(firstByte).length === 6 &&
      !completedEarly &&
      Object.values(servedSnapshot).every((s) => s.bytes > 0) &&
      windowRows >= 6 &&
      tcp.max <= 8 &&
      t1Permits.acquires > 0 &&
      t1Permits.max_held <= 8
      ? "passed"
      : "failed",
    {
      first_byte_ms: firstByte,
      served: servedSnapshot,
      window_rows_with_percent: windowRows,
      timeline,
      fixture_requests_so_far: t1Stats.log,
      engine_permits: t1Permits,
      client_tcp_established_max: tcp.max,
      client_tcp_samples: tcp.samples,
      host_overlap: t1Stats.overlap,
      raw_peak_including_unnoticed_closes: t1Stats.raw_peak,
    },
  );

  // T2: lowering the global limit while six run: none is interrupted; new work waits.
  const before = Object.fromEntries(ids.map((id) => [id, bytes(job(id))]));
  dm(["concurrency-set", "2"]);
  const late = dm(["add", url(NAMES[6])]).json?.data?.job_id;
  await sleep(15000);
  const after = Object.fromEntries(ids.map((id) => [id, job(id)]));
  const lateJob = job(late);
  record(
    "T2 lowering the global limit to 2 while six transfer: over 15 s all six keep running and gain bytes; a new download waits (queue_limit)",
    ids.every((id) => after[id]?.phase === "transferring" && bytes(after[id]) > before[id]) &&
      lateJob?.phase === "queued" &&
      (lateJob?.blockers ?? []).includes("queue_limit") &&
      bytes(lateJob) === 0
      ? "passed"
      : "failed",
    {
      phases: Object.fromEntries(ids.map((id) => [id, after[id]?.phase])),
      late: { phase: lateJob?.phase, blockers: lateJob?.blockers, bytes: bytes(lateJob) },
    },
  );

  // T3: a manual pause and a stopped queue survive raising the limit; eligible work starts at once.
  // "Paused" is the user's desired_state (the window derives its status from it); the evidence
  // that the transfer stopped is that neither the job's bytes nor the bytes served for its file
  // grow afterwards.
  dm(["pause", ids[0]]);
  await sleep(2000);
  const pausedBytes = bytes(job(ids[0]));
  const pausedServed = (await stats()).served[NAMES[0]].bytes;
  const q2 = dm(["queue-create", "Second", "--max-active", "6"]).json?.data?.queue_id;
  dm(["queue-stop", q2]);
  const held = dm(["add", url(NAMES[7])]).json?.data?.job_id;
  dm(["set-queue", held, q2]);
  await sleep(1000);
  const raisedAt = Date.now();
  dm(["concurrency-set", "16"]);
  const admitted = await until(
    "the waiting download to start after raising",
    () => job(late)?.phase === "transferring" && Date.now() - raisedAt,
    10_000,
  ).catch(() => null);
  await sleep(3000);
  const pausedJob = job(ids[0]);
  const heldJob = job(held);
  const pausedServedLater = (await stats()).served[NAMES[0]].bytes;
  record(
    "T3 raising the limit to 16 starts the waiting Main download at once; a manually paused download stays paused (no bytes received or served after the pause) and a download in a stopped queue stays held",
    admitted !== null &&
      pausedJob?.desired_state === "paused" &&
      bytes(pausedJob) === pausedBytes &&
      pausedServedLater === pausedServed &&
      heldJob?.phase === "queued" &&
      (heldJob?.blockers ?? []).includes("queue_stopped") &&
      bytes(heldJob) === 0
      ? "passed"
      : "failed",
    {
      admitted_after_ms: admitted,
      paused: {
        desired: pausedJob?.desired_state,
        bytes_at_pause: pausedBytes,
        bytes_later: bytes(pausedJob),
        served_at_pause: pausedServed,
        served_later: pausedServedLater,
      },
      held: { phase: heldJob?.phase, blockers: heldJob?.blockers },
    },
  );

  // R1: the service ends abruptly mid-transfer; a new service keeps both limits and recovers.
  const running = [...ids.slice(1), late];
  await until(
    "a committed checkpoint for every running download",
    () => running.every(traced),
    60_000,
  ).catch(() => null);
  const checkpointed = Object.fromEntries(running.map((id) => [id, traced(id)]));
  const midway = Object.fromEntries(running.map((id) => [id, bytes(job(id))]));
  kill(servicePid());
  await sleep(1500);
  const svc = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
    detached: true,
  });
  svc.unref();
  await until("the restarted service", () => dm(["hello"]).code === 0, 30_000);
  const limitAfter = dm(["concurrency"]).json?.data?.max_active_downloads;
  const mainAfter = (dm(["queues"]).json?.data?.queues ?? []).find((q) => q.queue_id === "main");
  const recovered = await until(
    "recovered downloads to transfer again",
    () => {
      const now = jobs();
      return [...ids.slice(1), late].every((id) => {
        const j = now.find((x) => x.job_id === id);
        return j && (j.phase === "transferring" || j.phase === "completed");
      })
        ? now
        : null;
    },
    60_000,
  ).catch(() => null);
  record(
    "R1 after an abrupt service stop mid-transfer and a restart: the global limit (16) and Main's limit (6) persist; interrupted downloads resume from their checkpoints; the pause and the stopped queue still hold",
    limitAfter === 16 &&
      mainAfter?.max_active === 6 &&
      recovered !== null &&
      job(ids[0])?.desired_state === "paused" &&
      job(held)?.phase === "queued"
      ? "passed"
      : "failed",
    {
      global: limitAfter,
      main: mainAfter?.max_active,
      checkpointed_before_stop: checkpointed,
      bytes_before_stop: midway,
      recovered: recovered !== null,
      states: Object.fromEntries(
        running.map((id) => {
          const j = job(id);
          return [id, { phase: j?.phase, bytes: bytes(j), error: j?.error?.kind ?? null }];
        }),
      ),
    },
  );

  // W1: the window reattached to the restarted service on its own: Options shows the persisted
  // limit and the list holds the service's downloads.
  const window = await page
    .ui(
      `(async () => {
    await until(() => !document.querySelector(".app-window__notice"), 30000);
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Options").click();
    const input = await until(() => labelled("Maximum active downloads"), 10000);
    await until(() => !input.disabled && input.value !== "", 15000);
    const value = input.value;
    [...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.trim() === "Close")?.click();
    await sleep(300);
    return { limit_in_window: value, rows: Number(document.querySelector("table")?.getAttribute("aria-rowcount") ?? 0) - 1 };
  })()`,
    )
    .catch((e) => ({ error: String(e) }));
  const serviceCount = jobs().length;
  record(
    "W1 after the restart the window reattached by itself: Options shows the persisted limit (16) and the list has the service's downloads",
    window?.limit_in_window === "16" && window.rows === serviceCount ? "passed" : "failed",
    { ...window, service_downloads: serviceCount },
  );

  // C1: cancelling a running download under contention: it ends cancelled, stops receiving and
  // being served bytes, leaves no final file, and the others keep transferring.
  const victim = dm(["add", url(NAMES[8])]).json?.data?.job_id;
  const victimStarted = await until(
    "the ninth download to receive bytes",
    () => bytes(job(victim)) > 0,
    60_000,
  ).catch(() => false);
  const othersBefore = Object.fromEntries(running.map((id) => [id, bytes(job(id))]));
  const cancelExit = dm(["cancel", victim]).code;
  await sleep(2000);
  const servedAtCancel = (await stats()).served[NAMES[8]].bytes;
  const bytesAtCancel = bytes(job(victim));
  await sleep(4000);
  const victimLater = job(victim);
  const servedLater = (await stats()).served[NAMES[8]].bytes;
  const othersAfter = Object.fromEntries(running.map((id) => [id, job(id)]));
  const finalFile = fs.existsSync(path.join(dl, NAMES[8]));
  record(
    "C1 cancelling a running download under contention: it ends cancelled, receives and is served no more bytes, leaves no final file, and the other downloads keep going",
    victimStarted !== false &&
      cancelExit === 0 &&
      victimLater?.phase === "cancelled" &&
      bytes(victimLater) === bytesAtCancel &&
      servedLater === servedAtCancel &&
      !finalFile &&
      running.some(
        (id) => othersAfter[id]?.phase === "completed" || bytes(othersAfter[id]) > othersBefore[id],
      )
      ? "passed"
      : "failed",
    {
      cancel_exit: cancelExit,
      phase: victimLater?.phase,
      desired: victimLater?.desired_state,
      bytes_at_cancel: bytesAtCancel,
      bytes_later: bytes(victimLater),
      served_at_cancel: servedAtCancel,
      served_later: servedLater,
      final_file_present: finalFile,
    },
  );

  // F1: release the holds, let everything finish, and compare every file byte for byte.
  dm(["resume", ids[0]]);
  dm(["queue-start", q2]);
  const all = [...ids, late, held];
  const done = await until(
    "all eight downloads to finish",
    () => {
      const now = jobs();
      const mine = all.map((id) => now.find((x) => x.job_id === id));
      return mine.every((j) => j && ["completed", "failed", "cancelled"].includes(j.phase))
        ? mine
        : null;
    },
    600_000,
  ).catch(() => null);
  const hashes = {};
  let exact = done !== null;
  for (const [i, id] of all.entries()) {
    const j = done?.find((x) => x.job_id === id);
    const name = [...NAMES.slice(0, 6), NAMES[6], NAMES[7]][i];
    const file = path.join(dl, j?.display_name ?? name);
    const got = fs.existsSync(file) ? sha256(fs.readFileSync(file)) : null;
    hashes[name] = {
      phase: j?.phase,
      match: got === expected[name],
      error: j?.error ?? null,
    };
    if (j?.phase !== "completed" || got !== expected[name]) exact = false;
  }
  clearInterval(tcpTimer);
  tcpTimer = null;
  const f1Stats = await stats();
  const f1Permits = permits();
  record(
    "F1 all eight downloads complete and every file matches its SHA-256; the host never had more than 8 requests open at once (across the abrupt stop and restart) and no file more than 4 range requests",
    exact &&
      tcp.max <= 8 &&
      f1Permits.acquires > 0 &&
      f1Permits.max_held <= 8 &&
      Object.values(f1Stats.per_file_overlap).every((n) => n <= 4)
      ? "passed"
      : "failed",
    {
      hashes,
      engine_permits: f1Permits,
      client_tcp_established_max: tcp.max,
      client_tcp_samples: tcp.samples,
      client_tcp_at_max: tcp.at_max,
      host_overlap: f1Stats.overlap,
      per_file_range_overlap: f1Stats.per_file_overlap,
      raw_peak_including_unnoticed_closes: f1Stats.raw_peak,
      requests_per_file: Object.fromEntries(NAMES.map((n) => [n, f1Stats.served[n].requests])),
    },
  );
} catch (e) {
  record("run", "failed", { error: String(e?.stack ?? e) });
} finally {
  page?.close();
  kill(desktop.pid, true);
  kill(servicePid(), true);
  if (tcpTimer) clearInterval(tcpTimer);
  fixture.kill();
  await sleep(1500);
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    /* a just-ended process may still hold a file; the folder is in TEMP */
  }
}
finish();

/** DevTools over the WebView's debugging port; `ui(expr)` runs with small page helpers. */
async function devtools(p) {
  let target;
  for (let i = 0; i < 150 && !target; i++) {
    try {
      const list = await (
        await fetch(`http://127.0.0.1:${p}/json`, { signal: AbortSignal.timeout(5000) })
      ).json();
      target = list.find((t) => t.type === "page");
    } catch {
      /* not up yet */
    }
    if (!target) await sleep(200);
  }
  if (!target) throw new Error("no WebView DevTools page");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  const ipc = [];
  await new Promise((r, j) => {
    ws.onopen = r;
    ws.onerror = j;
  });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (
      msg.method === "Network.requestWillBeSent" &&
      /ipc\.localhost|^ipc:/.test(msg.params.request.url)
    )
      ipc.push(`${msg.params.request.method} ${msg.params.request.url}`);
    waiting.get(msg.id)?.(msg);
    waiting.delete(msg.id);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => reject(new Error(`${method} timed out`)), 60_000);
      waiting.set(n, (msg) => {
        clearTimeout(timer);
        if (msg.error) reject(new Error(msg.error.message));
        else resolve(msg.result);
      });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const HELPERS = `
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const until = async (fn, ms) => { const t0 = performance.now(); while (performance.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(50); } throw new Error("page wait timed out"); };
    const labelled = (text) => { const l = [...document.querySelectorAll("label")].find((x) => x.textContent.trim() === text); return l ? document.getElementById(l.htmlFor) : null; };
    const setValue = (el, v) => { const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };`;
  const ui = async (expr) => {
    // The page may still be loading when DevTools attaches: retry a destroyed context.
    for (let k = 0; k < 5; k++) {
      const r = await send("Runtime.evaluate", {
        expression: `(async () => { ${HELPERS} return await (${expr}); })()`,
        awaitPromise: true,
        returnByValue: true,
      });
      if (r.exceptionDetails) {
        const text = r.exceptionDetails.exception?.description ?? r.exceptionDetails.text;
        if (/context was destroyed|Cannot find context/i.test(text)) {
          await sleep(1000);
          continue;
        }
        throw new Error(text);
      }
      return r.result?.value;
    }
    throw new Error("the page kept reloading");
  };
  return { send, ui, ipc, close: () => ws.close() };
}
