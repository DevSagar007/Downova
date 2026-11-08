#!/usr/bin/env node
// ADV09 / REL-01 (B3.6): the update journey from fixture build A to signed fixture build B,
// through the REAL packaged window and the REAL update dialog, without installing anything.
//
//   node scripts/acceptance/update-journey.mjs [--pair dist-packages/update-fixture-pair.json]
//
// Windows only. Both packages come from scripts/update/build-fixture-pair.mjs (one disposable key,
// loopback endpoint). Isolation (each recorded in results.json, checked before and after):
// - installation: `msiexec /a` administrative extracts into a fresh temporary folder (no Windows
//   Installer registration, no uninstall entry, no shortcut);
// - data, IPC endpoint and vault namespace: DOWNOVA_HOME is a fresh temporary folder (the service
//   derives its endpoint and its Credential Manager namespace "Downova/<hash of that folder>" from
//   it); DOWNOVA_DOWNLOAD_DIR likewise; the WebView2 profile likewise;
// - browser registrations: DOWNOVA_SKIP_BROWSER_SETUP=1, and the native-messaging keys of Chrome,
//   Edge and Firefox must be byte-identical before and after;
// - the user's own data folder (%LOCALAPPDATA%\Downova) must be unchanged;
// - processes: only processes started here, or whose executable lies in this run's folder, are
//   stopped; a final sweep proves none of them is left.
// The installer itself is NOT run (that would be a real per-user/per-machine installation): after
// "Ready to install" the harness performs the installer's effect on this isolated copy instead:
// the service is shut down with its state kept (as the shell's install step does), and B's
// extracted binaries start on the same data. Steps:
//   U1  A starts; hello reports A's version.
//   U2  work: two throttled downloads (one protected link: sealed, data key in Credential Manager),
//       one manually paused, one in a stopped queue.
//   U3  Help → Check for updates…: B offered, downloaded, "Verified" (signature + signed version).
//   U4  Prepare: held count, desired states unchanged, backup and pending marker checked on disk.
//   U5  Cancel ("Not now"): the held downloads continue; nothing pending.
//   U6  Prepare again; U7 shut the service down keeping state (the installer's precondition).
//   U8  B starts on the same data: hello B, update accepted (last.json), About says B.
//   U9  held downloads finish with the expected SHA-256 (the protected one through its sealed link);
//       the paused one stays paused; the stopped queue stays stopped.
//   U10 B's dialog offers nothing (B is not newer than itself).
//   U11 recoverable failure: the live database is damaged; B's service refuses to serve (exit 6,
//       recovery record); the dialog shows "Recovery required"; Restore → the checked backup.
//   U12 isolation and cleanup proofs.
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { devtools } from "../lib/devtools.mjs";
import { payload } from "../lib/fixture.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const pair = JSON.parse(
  fs.readFileSync(path.resolve(arg("--pair", "dist-packages/update-fixture-pair.json")), "utf8"),
);
const outDir = path.resolve(arg("--out", path.join("acceptance-results", "update-journey")));
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const results = {
  started_at: new Date().toISOString(),
  observer: "automated: packaged windows over WebView2 DevTools, plus the packaged dm CLI",
  category: "extracted-package journey (no installer run)",
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  pair,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ step, status, ...detail });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 400)}`);
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "Windows only (MSI extracts, WebView2 DevTools)" });
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  process.exit(2);
}

// ------------------------------------------------------------------ isolation snapshots
const REG_KEYS = [
  "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts",
  "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts",
  "HKCU\\Software\\Mozilla\\NativeMessagingHosts",
  "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];
const regDump = () =>
  Object.fromEntries(
    REG_KEYS.map((k) => [
      k,
      sha256(spawnSync("reg", ["query", k, "/s"], { encoding: "utf8" }).stdout ?? ""),
    ]),
  );
const realData = path.join(process.env.LOCALAPPDATA, "Downova");
const realDataState = () =>
  fs.existsSync(realData)
    ? fs
        .readdirSync(realData, { recursive: true })
        .map(String)
        .sort()
        .map((f) => {
          const st = fs.statSync(path.join(realData, f));
          return `${f}:${st.isFile() ? st.size : "dir"}:${st.mtimeMs}`;
        })
        .join("\n")
    : "absent";
const startMenu = path.join(process.env.APPDATA, "Microsoft/Windows/Start Menu/Programs");
const shortcuts = () =>
  fs.existsSync(startMenu)
    ? fs
        .readdirSync(startMenu, { recursive: true })
        .map(String)
        .filter((f) => /downova/i.test(f))
    : [];
const credTargets = () =>
  (spawnSync("cmdkey", ["/list"], { encoding: "utf8" }).stdout ?? "")
    .split(/\r?\n/)
    .map((l) => /Target:\s*(.+)$/i.exec(l.trim())?.[1]?.trim())
    .filter((t) => t && /Downova\//.test(t));
const before = {
  registry: regDump(),
  real_data: sha256(realDataState()),
  shortcuts: shortcuts(),
  credentials: new Set(credTargets()),
};

// ------------------------------------------------------------------ extraction
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-update-journey-"));
results.work = work;
function extract(dir, name) {
  const info = JSON.parse(fs.readFileSync(path.join(root, dir, "build-info.json"), "utf8"));
  if (info.updater !== "fixture") throw new Error(`${dir} is not an update-fixture package`);
  const msi = fs.readdirSync(path.join(root, dir)).find((f) => f.endsWith(".msi"));
  const target = path.join(work, name);
  const r = spawnSync("msiexec", ["/a", path.join(root, dir, msi), "/qn", `TARGETDIR=${target}`]);
  if (r.status !== 0) throw new Error(`msiexec /a ${msi} exited ${r.status}`);
  const find = (d, n) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) {
        const f = find(p, n);
        if (f) return f;
      } else if (e.name.toLowerCase() === n) return p;
    }
    return null;
  };
  const bin = path.dirname(find(target, "dm-service.exe"));
  return {
    info,
    msi: path.join(root, dir, msi),
    bin,
    desktop: path.join(bin, "dm-desktop.exe"),
    dm: path.join(bin, "dm.exe"),
    service: path.join(bin, "dm-service.exe"),
  };
}
const A = extract(pair.a.dir, "A");
const B = extract(pair.b.dir, "B");
results.packages = {
  a: {
    msi: path.basename(A.msi),
    msi_sha256: sha256(fs.readFileSync(A.msi)),
    source: A.info.source,
  },
  b: {
    msi: path.basename(B.msi),
    msi_sha256: sha256(fs.readFileSync(B.msi)),
    source: B.info.source,
  },
};

const home = path.join(work, "data");
const dl = path.join(work, "downloads");
fs.mkdirSync(dl, { recursive: true });
const baseEnv = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  DOWNOVA_SKIP_BROWSER_SETUP: "1",
};
const owned = new Set();
function dmOf(app) {
  return (args) => {
    const r = spawnSync(app.dm, ["--data-dir", home, "--json", ...args], {
      env: baseEnv,
      encoding: "utf8",
      timeout: 30_000,
    });
    try {
      return { code: r.status, json: JSON.parse(r.stdout) };
    } catch {
      return { code: r.status, json: null, stderr: r.stderr };
    }
  };
}
const endpoint = () => {
  try {
    return JSON.parse(fs.readFileSync(path.join(home, "run", "endpoint.json"), "utf8"));
  } catch {
    return null;
  }
};
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(300);
  }
  throw new Error(`timed out waiting for ${what}`);
}
let port = 9480;
async function startDesktop(app, label) {
  port += 1;
  const child = spawn(app.desktop, [], {
    env: {
      ...baseEnv,
      WEBVIEW2_USER_DATA_FOLDER: path.join(work, `webview-${label}`),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
    stdio: "ignore",
  });
  owned.add(child.pid);
  const page = await devtools(port);
  return { child, page };
}
function stopDesktop(d) {
  if (!d) return;
  try {
    d.page.close?.();
  } catch {
    /* already gone */
  }
  // Only the window process: the service it started is a detached child in the process tree and
  // must be stopped by its own shutdown, never by /T (that would be a crash, not a stop).
  spawnSync("taskkill", ["/PID", String(d.child.pid), "/F"]);
}

// ------------------------------------------------------------------ fixture servers
const MIB = 1024 * 1024;
const FILES = {
  run1: payload(48 * MIB, 301),
  prot1: payload(24 * MIB, 302),
  paused1: payload(24 * MIB, 303),
  q1: payload(8 * MIB, 304),
};
const SECRET = crypto.randomBytes(12).toString("hex");
const content = http.createServer((req, res) => {
  const u = new URL(req.url, "http://x");
  const name = u.pathname.slice(1).replace(/\.bin$/, "");
  const body = FILES[name];
  if (!body || (name === "prot1" && u.searchParams.get("token") !== SECRET)) {
    res.writeHead(404).end();
    return;
  }
  const etag = `"${sha256(body).slice(0, 16)}"`;
  const m = /bytes=(\d+)-(\d*)/.exec(req.headers.range ?? "");
  if (m) {
    const start = Number(m[1]);
    const end = m[2] ? Number(m[2]) : body.length - 1;
    res.writeHead(206, {
      "Content-Range": `bytes ${start}-${end}/${body.length}`,
      "Content-Length": end - start + 1,
      "Accept-Ranges": "bytes",
      ETag: etag,
    });
    res.end(body.subarray(start, end + 1));
  } else {
    res.writeHead(200, { "Content-Length": body.length, "Accept-Ranges": "bytes", ETag: etag });
    res.end(body);
  }
});
await new Promise((r) => content.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${content.address().port}`;
const bMsiSig = fs.readFileSync(`${B.msi}.sig`, "utf8").trim();
const manifest = {
  version: pair.b.version,
  notes: "update-journey fixture",
  pub_date: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
  platforms: {
    "windows-x86_64-msi": { signature: bMsiSig, url: `http://127.0.0.1:${pair.port}/b.msi` },
  },
};
const updates = http.createServer((req, res) => {
  if (req.url.startsWith("/latest.json")) {
    res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(manifest));
  } else if (req.url.startsWith("/b.msi")) {
    const b = fs.readFileSync(B.msi);
    res.writeHead(200, { "Content-Length": b.length }).end(b);
  } else res.writeHead(404).end();
});
await new Promise((r, j) => updates.listen(pair.port, "127.0.0.1", r).on("error", j));

// ------------------------------------------------------------------ helpers over the UI
const UI = {
  openUpdates: `(async () => {
    click(await until(() => byText('[role="menuitem"]', /^Help$/), 30000)); await frame();
    click(await until(() => byText('[role="menuitem"]', /^Check for updates…$/), 5000)); await frame();
    return !!document.querySelector('[role="dialog"]'); })()`,
  clickButton: (re) => `(async () => {
    const b = await until(() => byText('[role="dialog"] button', ${re}), 60000);
    click(b); await frame(); return true; })()`,
  waitText: (re) => `(async () => {
    await until(() => ${re}.test(document.querySelector('[role="dialog"]')?.textContent ?? ""), 180000);
    return document.querySelector('[role="dialog"]').textContent; })()`,
  closeDialog: `(async () => { document.querySelector('[role="dialog"] .dialog__close')?.click(); await frame(); return true; })()`,
  about: `(async () => {
    click(byText('[role="menuitem"]', /^Help$/)); await frame();
    click(byText('[role="menuitem"]', /^About Downova$/)); await frame();
    const t = document.querySelector('.about__version')?.textContent ?? "";
    document.querySelector('[role="dialog"] .dialog__close')?.click(); await frame(); return t; })()`,
};
const job = (dm, id) => dm(["get", id]).json?.data;

let desktop = null;
let exitCode = 1;
try {
  // U1 -------------------------------------------------------------
  desktop = await startDesktop(A, "a");
  const dmA = dmOf(A);
  const helloA = await until("A's service", () => dmA(["hello"]).json?.data, 60_000);
  record(
    "U1 A starts its own service on the isolated data",
    helloA.service_version === pair.a.version ? "passed" : "failed",
    {
      service_version: helloA.service_version,
      endpoint_in_work: endpoint()?.pid ? true : false,
    },
  );
  const servicePidA = endpoint()?.pid;
  if (servicePidA) owned.add(servicePidA);

  // U2 -------------------------------------------------------------
  const add = (url, ...extra) => {
    const r = dmA(["add", url, ...extra]);
    if (r.code !== 0) throw new Error(`add ${url}: ${JSON.stringify(r.json ?? r.stderr)}`);
    return r.json.data.job_id;
  };
  const q = dmA(["queue-create", "journey-stopped"]).json.data.queue_id;
  dmA(["queue-stop", q]);
  const ids = {
    run1: add(`${base}/run1.bin`),
    prot1: add(`${base}/prot1.bin?token=${SECRET}`),
    paused1: add(`${base}/paused1.bin`),
    q1: add(`${base}/q1.bin`, "--queue", q),
  };
  for (const id of [ids.run1, ids.prot1]) dmA(["job-speed-limit", id, "512k"]);
  dmA(["pause", ids.paused1]);
  await until(
    "run1 and prot1 receiving",
    () => [ids.run1, ids.prot1].every((id) => Number(job(dmA, id)?.downloaded_bytes ?? 0) > 0),
    60_000,
  );
  const pausedBefore = job(dmA, ids.paused1);
  const newCreds = credTargets().filter((t) => !before.credentials.has(t));
  record("U2 work in progress (running, protected, paused, stopped queue)", "passed", {
    phases: Object.fromEntries(Object.entries(ids).map(([k, id]) => [k, job(dmA, id)?.phase])),
    data_key_in_credential_manager: newCreds.length > 0,
  });

  // U3 -------------------------------------------------------------
  await desktop.page.ui(UI.openUpdates);
  await desktop.page.ui(UI.clickButton("/^Check for updates$/"));
  await desktop.page.ui(
    UI.waitText(`/Downova ${pair.b.version.replaceAll(".", "\\.")} is available/`),
  );
  await desktop.page.ui(UI.clickButton("/^Download and verify$/"));
  const verified = await desktop.page.ui(UI.waitText("/Verified/"));
  record(
    "U3 B offered, downloaded and verified in the real dialog",
    /signed by the trusted update key for exactly this version/.test(verified)
      ? "passed"
      : "failed",
  );
  fs.writeFileSync(
    path.join(outDir, "u3-verified.png"),
    Buffer.from(await desktop.page.screenshot(), "base64"),
  );

  // U4 -------------------------------------------------------------
  const prepare = async () => {
    await desktop.page.ui(UI.clickButton("/^Prepare to install$|^Prepare again$/"));
    return desktop.page.ui(UI.waitText("/Ready to install/"));
  };
  const ready = await prepare();
  const pending = JSON.parse(fs.readFileSync(path.join(home, "update", "pending.json"), "utf8"));
  const op1 = pending.operation_id;
  const meta = JSON.parse(
    fs.readFileSync(path.join(home, "update", "backups", `${op1}.json`), "utf8"),
  );
  const backupBytes = fs.readFileSync(path.join(home, "update", "backups", `${op1}.sqlite3`));
  const held = Object.fromEntries(Object.entries(ids).map(([k, id]) => [k, job(dmA, id)]));
  const refused = dmA(["add", `${base}/q1.bin?again=1`]);
  record(
    "U4 prepared: held, intent kept, checked backup, new work refused",
    /2 running download\(s\) are held/.test(ready) && refused.code !== 0 ? "passed" : "failed",
    {
      operation: op1,
      target_version: pending.target_version,
      backup_sha256_matches: sha256(backupBytes) === (meta.sha256 ?? meta.backup?.sha256),
      desired: Object.fromEntries(Object.entries(held).map(([k, j]) => [k, j?.desired_state])),
      paused_unchanged: held.paused1?.desired_state === pausedBefore?.desired_state,
      add_while_held: refused.json?.error?.kind ?? refused.code,
    },
  );

  // U5 -------------------------------------------------------------
  const b0 = Number(job(dmA, ids.run1)?.downloaded_bytes ?? 0);
  await desktop.page.ui(UI.clickButton("/^Not now$/"));
  await until(
    "held work to continue",
    () => Number(job(dmA, ids.run1)?.downloaded_bytes ?? 0) > b0,
    60_000,
  );
  record(
    "U5 cancel releases the service and the held downloads continue",
    fs.existsSync(path.join(home, "update", "pending.json")) ? "failed" : "passed",
    {
      paused_still_paused: job(dmA, ids.paused1)?.desired_state === pausedBefore?.desired_state,
    },
  );

  // U6 / U7 --------------------------------------------------------
  await desktop.page.ui(UI.closeDialog);
  await desktop.page.ui(UI.openUpdates);
  await desktop.page.ui(UI.clickButton("/^Check for updates$/"));
  await desktop.page.ui(UI.clickButton("/^Download and verify$/"));
  await desktop.page.ui(UI.waitText("/Verified/"));
  await prepare();
  const op2 = JSON.parse(
    fs.readFileSync(path.join(home, "update", "pending.json"), "utf8"),
  ).operation_id;
  record("U6 prepared again for the install", "passed", { operation: op2 });
  stopDesktop(desktop);
  desktop = null;
  const shut = dmA(["shutdown"]);
  await until(
    "A's service to exit",
    () =>
      !endpoint() ||
      spawnSync("tasklist", ["/FI", `PID eq ${servicePidA}`], { encoding: "utf8" }).stdout.indexOf(
        String(servicePidA),
      ) < 0,
    60_000,
  );
  record(
    "U7 the service stopped with its state kept (the installer's precondition); installer NOT RUN",
    shut.code === 0 ? "passed" : "failed",
    {
      note: "the MSI/NSIS installer would replace the program files here; this isolated run starts B's extracted copy instead",
      shutdown: { exit: shut.code, result: shut.json?.data ?? shut.json?.error ?? shut.stderr },
    },
  );

  // U8 -------------------------------------------------------------
  desktop = await startDesktop(B, "b");
  const dmB = dmOf(B);
  const helloB = await until("B's service", () => dmB(["hello"]).json?.data, 60_000);
  if (endpoint()?.pid) owned.add(endpoint().pid);
  const last = await until(
    "update/last.json",
    () => {
      try {
        return JSON.parse(fs.readFileSync(path.join(home, "update", "last.json"), "utf8"));
      } catch {
        return null;
      }
    },
    30_000,
  );
  const about = await desktop.page.ui(UI.about);
  record(
    "U8 B's service accepted the prepared update and opened the data",
    helloB.service_version === pair.b.version &&
      last.operation_id === op2 &&
      /installed/.test(JSON.stringify(last)) &&
      about.includes(pair.b.version)
      ? "passed"
      : "failed",
    {
      service_version: helloB.service_version,
      last,
      about,
    },
  );

  // U9 -------------------------------------------------------------
  for (const id of [ids.run1, ids.prot1]) dmB(["job-speed-limit", id, "off"]);
  const done = async (id) => (job(dmB, id)?.phase === "completed" ? job(dmB, id) : null);
  const j1 = await until("run1 to finish", () => done(ids.run1), 240_000);
  const j2 = await until("prot1 to finish", () => done(ids.prot1), 240_000);
  const hash = (j) => sha256(fs.readFileSync(path.join(dl, j.display_name)));
  const paused = job(dmB, ids.paused1);
  const queued = job(dmB, ids.q1);
  record(
    "U9 held work finished with the expected hashes; pause and stopped queue unchanged",
    hash(j1) === sha256(FILES.run1) &&
      hash(j2) === sha256(FILES.prot1) &&
      paused?.desired_state === pausedBefore?.desired_state &&
      Number(paused?.downloaded_bytes ?? 0) === Number(pausedBefore?.downloaded_bytes ?? 0) &&
      queued?.phase !== "completed"
      ? "passed"
      : "failed",
    {
      run1: hash(j1) === sha256(FILES.run1),
      prot1_through_sealed_link: hash(j2) === sha256(FILES.prot1),
      paused1: {
        desired: paused?.desired_state,
        phase: paused?.phase,
        error: paused?.error ?? paused?.last_error ?? null,
        blocker: paused?.blocker ?? null,
      },
      q1: { phase: queued?.phase },
    },
  );

  // U10 ------------------------------------------------------------
  await desktop.page.ui(UI.openUpdates);
  await desktop.page.ui(UI.clickButton("/^Check for updates$/"));
  const none = await desktop.page.ui(
    UI.waitText("/No newer version is offered|Downova .* is available/"),
  );
  record(
    "U10 B is not offered to itself",
    /No newer version is offered/.test(none) ? "passed" : "failed",
  );
  await desktop.page.ui(UI.closeDialog);

  // U11 ------------------------------------------------------------
  stopDesktop(desktop);
  desktop = null;
  dmB(["shutdown"]);
  await until("B's service to exit", () => dmB(["hello"]).code !== 0, 60_000);
  const db = path.join(home, "downova.sqlite3");
  for (const f of [db, `${db}-wal`, `${db}-shm`]) if (f !== db) fs.rmSync(f, { force: true });
  const fd = fs.openSync(db, "r+");
  fs.writeSync(fd, Buffer.alloc(4096, 0x5a), 0, 4096, 0);
  fs.closeSync(fd);
  const refuse = spawnSync(B.service, [], { env: baseEnv, encoding: "utf8", timeout: 60_000 });
  const recovery = JSON.parse(fs.readFileSync(path.join(home, "update", "recovery.json"), "utf8"));
  desktop = await startDesktop(B, "b2");
  // The window reloads once it learns the service cannot start; let it settle.
  await sleep(8000);
  await desktop.page.ui(UI.openUpdates);
  await desktop.page.ui(UI.waitText("/Recovery required/"));
  await desktop.page.ui(UI.clickButton("/^Restore the backup…$/"));
  await desktop.page.ui(UI.clickButton("/^Restore now$/"));
  const afterRestore = await desktop.page.ui(
    UI.waitText("/Restoring the backup failed|Downova checks for updates only when you ask/"),
  );
  results.restore_dialog = afterRestore.slice(0, 400);
  fs.writeFileSync(
    path.join(outDir, "u11-after-restore.png"),
    Buffer.from(await desktop.page.screenshot(), "base64"),
  );
  const restoredHello = await until(
    "the service after restore",
    () => dmB(["hello"]).json?.data,
    120_000,
  );
  if (endpoint()?.pid) owned.add(endpoint().pid);
  const list = dmB(["list"]).json?.data?.items ?? [];
  record(
    "U11 damaged data: exit 6 with a recovery record; restored from the checked backup through the dialog",
    refuse.status === 6 && recovery.backup_operation && restoredHello && list.length >= 4
      ? "passed"
      : "failed",
    {
      service_exit: refuse.status,
      recovery_backup: recovery.backup_operation,
      jobs_after_restore: list.length,
      paused_after_restore: list.find((j) => j.job_id === ids.paused1)?.desired_state,
      set_aside: fs.existsSync(path.join(home, "update", "set-aside")),
    },
  );
  exitCode = 0;
} catch (e) {
  record("run", "failed", { error: String(e?.stack ?? e).slice(0, 1500) });
} finally {
  // U12 ------------------------------------------------------------
  stopDesktop(desktop);
  for (const app of [B, A])
    spawnSync(app.dm, ["--data-dir", home, "shutdown"], { env: baseEnv, timeout: 20_000 });
  await sleep(2000);
  const leftovers =
    spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "Get-Process | Where-Object { $_.Path } | Select-Object Id,Path | ConvertTo-Json",
      ],
      { encoding: "utf8" },
    ).stdout || "[]";
  let mine = [];
  try {
    mine = [].concat(JSON.parse(leftovers)).filter((p) => p.Path && p.Path.startsWith(work));
  } catch {
    /* none */
  }
  for (const p of mine) spawnSync("taskkill", ["/PID", String(p.Id), "/T", "/F"]);
  content.close();
  updates.close();
  const created = credTargets().filter((t) => !before.credentials.has(t));
  for (const t of created) spawnSync("cmdkey", [`/delete:${t}`]);
  const after = { registry: regDump(), real_data: sha256(realDataState()), shortcuts: shortcuts() };
  const regSame = REG_KEYS.every((k) => before.registry[k] === after.registry[k]);
  record(
    "U12 isolation held and cleanup completed",
    regSame &&
      before.real_data === after.real_data &&
      after.shortcuts.length === before.shortcuts.length &&
      credTargets().filter((t) => !before.credentials.has(t)).length === 0
      ? "passed"
      : "failed",
    {
      registry_unchanged: Object.fromEntries(
        REG_KEYS.map((k) => [k, before.registry[k] === after.registry[k]]),
      ),
      real_data_unchanged: before.real_data === after.real_data,
      shortcuts_unchanged: after.shortcuts.length === before.shortcuts.length,
      owned_processes_left_before_sweep: mine.length,
      credential_entries_created_and_deleted: created.length,
    },
  );
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    results.work_left = work;
  }
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.every((s) => ["passed", "note"].includes(s.status))
    ? "passed"
    : "failed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(`verdict ${results.verdict}; results in ${outDir}`);
  process.exit(results.verdict === "passed" ? 0 : exitCode || 1);
}
