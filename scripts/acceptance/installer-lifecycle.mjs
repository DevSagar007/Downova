#!/usr/bin/env node
// REL-01 (beta readiness): the native installer and upgrade lifecycle with REAL installers.
//
//   node scripts/acceptance/installer-lifecycle.mjs [--pair dist-packages/update-fixture-pair.json]
//        [--out acceptance-results/installer-lifecycle]
//
// It installs, upgrades and removes Downova for real, so it runs ONLY in a disposable machine:
// a GitHub-hosted runner (GITHUB_ACTIONS=true and RUNNER_ENVIRONMENT=github-hosted), or a VM the
// operator declares disposable with DOWNOVA_DISPOSABLE_MACHINE=1. It also refuses to start if
// Downova is already installed or has data for this account. It never kills processes by name:
// only processes whose executable lies in the installation folder, or that it started itself.
//
// Packages: the update-fixture pair (scripts/update/build-fixture-pair.mjs): A and B are real,
// version-consistent installers of two versions. Rows (each recorded with exit codes, versions
// and file outcomes; results.json):
//   Windows NSIS (per user): N1 install A, first start; N2 work (running, manually paused, stopped
//     queue) and an unrelated browser registration; N3 upgrade to B while the service runs (the
//     installer stops only its own service); N4 B continues the work (SHA-256), the pause and the
//     stopped queue kept; N5 uninstall while the service runs: no owned process left, data and
//     downloads kept, only Downova's registration removed; N6 reinstall: the retained list is back.
//   Windows MSI (installs per user to %LOCALAPPDATA%\Downova, like NSIS): M1 install A; M2 upgrade
//     to B with the service running (exit code recorded: MSI has no Downova hooks); M3 B continues
//     the work; M4 uninstall: data and downloads kept.
//   Linux .deb: D1 install A (dpkg); D2 work; D3 upgrade to B with dpkg while the service runs;
//     D4 B continues the work; D5 remove: data and downloads kept.
//   macOS .app: A1 copy A to /Applications, work; A2 replace with B (the service stopped first),
//     B continues; A3 remove the app: data and downloads kept.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { payload } from "../lib/fixture.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const outDir = path.resolve(arg("--out", "acceptance-results/installer-lifecycle"));
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const results = {
  started_at: new Date().toISOString(),
  category: "installed package (real installers on a disposable machine)",
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ step, status, ...detail });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 600)}`);
};
const finish = () => {
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.every((s) => ["passed", "note", "not run"].includes(s.status))
    ? "passed"
    : "failed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(`verdict ${results.verdict}; results in ${outDir}`);
  process.exit(results.verdict === "passed" ? 0 : 1);
};

// ------------------------------------------------------------------ isolation guard
const hosted =
  process.env.GITHUB_ACTIONS === "true" && process.env.RUNNER_ENVIRONMENT === "github-hosted";
const declared = process.env.DOWNOVA_DISPOSABLE_MACHINE === "1";
results.isolation = {
  github_hosted_runner: hosted,
  declared_disposable_vm: declared,
  runner_image: process.env.ImageOS ?? null,
};
if (!hosted && !declared) {
  record("isolation", "failed", {
    reason: "refused: not a GitHub-hosted runner and not declared a disposable machine",
  });
  finish();
}
const platform = process.platform;
const home = os.homedir();
const dataDir =
  platform === "win32"
    ? path.join(process.env.LOCALAPPDATA, "Downova")
    : platform === "darwin"
      ? path.join(home, "Library/Application Support/Downova")
      : path.join(process.env.XDG_DATA_HOME ?? path.join(home, ".local/share"), "downova");
const pair = JSON.parse(
  fs.readFileSync(path.resolve(arg("--pair", "dist-packages/update-fixture-pair.json")), "utf8"),
);
const pkg = (which, re) => {
  const dir = path.join(root, pair[which].dir);
  const f = fs.readdirSync(dir).find((n) => re.test(n));
  if (!f) throw new Error(`no ${re} in ${dir}`);
  return { file: path.join(dir, f), name: f, sha256: sha256(fs.readFileSync(path.join(dir, f))) };
};

// ------------------------------------------------------------------ helpers
const exe = platform === "win32" ? ".exe" : "";
let binDir = null;
const owned = new Set();
const downloads = fs.mkdtempSync(path.join(os.tmpdir(), "downova-lifecycle-dl-"));
function dm(args) {
  const r = spawnSync(path.join(binDir, `dm${exe}`), ["--json", ...args], {
    encoding: "utf8",
    timeout: 60_000,
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null, stderr: (r.stderr ?? "").slice(0, 300) };
  }
}
const versionOf = (bin) => {
  const r = spawnSync(path.join(binDir, `${bin}${exe}`), ["--version"], { encoding: "utf8" });
  return /\b(\d+\.\d+\.\d+)\b/.exec(r.stdout ?? "")?.[1] ?? null;
};
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${what}`);
}
let serviceRuns = 0;
let lastServiceLog = null;
/** Starts the installed service; its output goes to a log in the results folder. */
function startService() {
  serviceRuns += 1;
  lastServiceLog = path.join(outDir, `service-${serviceRuns}.log`);
  const out = fs.openSync(lastServiceLog, "a");
  const child = spawn(path.join(binDir, `dm-service${exe}`), ["--download-dir", downloads], {
    detached: true,
    stdio: ["ignore", out, out],
  });
  child.unref();
  fs.closeSync(out);
  owned.add(child.pid);
  return child.pid;
}
/** The last lines of the most recent service log (diagnostics for a failed row). */
const serviceLogTail = (n = 400) => {
  try {
    return fs.readFileSync(lastServiceLog, "utf8").slice(-n);
  } catch {
    return null;
  }
};
/** What `dm hello` answers: version on success, otherwise the exit code and stderr. */
const helloState = () => {
  const r = dm(["hello"]);
  return r.json?.data
    ? { ok: true, version: r.json.data.service_version ?? r.json.data.version ?? null }
    : { ok: false, code: r.code, error: r.json?.error ?? r.stderr ?? null };
};
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};
/** Processes whose executable is inside `dir` (never matched by name). */
function processesIn(dir) {
  const want = path.resolve(dir).toLowerCase();
  if (platform === "win32") {
    const out = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "Get-Process | Where-Object { $_.Path } | Select-Object Id,Path | ConvertTo-Json -Compress",
      ],
      { encoding: "utf8" },
    ).stdout;
    try {
      return []
        .concat(JSON.parse(out || "[]"))
        .filter((p) => p.Path && path.resolve(p.Path).toLowerCase().startsWith(want))
        .map((p) => ({ pid: p.Id, path: p.Path }));
    } catch {
      return [];
    }
  }
  const out = spawnSync("ps", ["-axo", "pid=,comm="], { encoding: "utf8" }).stdout ?? "";
  return out
    .split("\n")
    .map((l) => l.trim().split(/\s+(.+)/))
    .filter(([pid, p]) => pid && p && path.resolve(p).toLowerCase().startsWith(want))
    .map(([pid, p]) => ({ pid: Number(pid), path: p }));
}
const job = (id) => dm(["get", id]).json?.data;

// ------------------------------------------------------------------ content (paced, owned)
const MIB = 1024 * 1024;
const FILES = {
  run1: payload(24 * MIB, 501),
  paused1: payload(8 * MIB, 502),
  q1: payload(4 * MIB, 503),
};
let slow = true;
const server = http.createServer(async (req, res) => {
  const name = req.url.slice(1).replace(/\.bin.*$/, "");
  const body = FILES[name];
  if (!body) return res.writeHead(404).end();
  const etag = `"${sha256(body).slice(0, 16)}"`;
  const m = /bytes=(\d+)-(\d*)/.exec(req.headers.range ?? "");
  const start = m ? Number(m[1]) : 0;
  const end = m && m[2] ? Number(m[2]) : body.length - 1;
  res.writeHead(m ? 206 : 200, {
    "Content-Length": end - start + 1,
    "Accept-Ranges": "bytes",
    ETag: etag,
    ...(m ? { "Content-Range": `bytes ${start}-${end}/${body.length}` } : {}),
  });
  for (let i = start; i <= end && !res.destroyed; i += 64 * 1024) {
    res.write(body.subarray(i, Math.min(i + 64 * 1024, end + 1)));
    if (slow) await sleep(100);
  }
  res.end();
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

/** Work that must survive an upgrade: running, manually paused, in a stopped queue. */
async function startWork(prefix) {
  const add = (url, ...x) => {
    const r = dm(["add", url, ...x]);
    if (r.code !== 0) throw new Error(`add: ${JSON.stringify(r.json ?? r.stderr)}`);
    return r.json.data.job_id;
  };
  const q = dm(["queue-create", `${prefix}-stopped`]).json.data.queue_id;
  dm(["queue-stop", q]);
  const ids = {
    run1: add(`${base}/run1.bin`),
    paused1: add(`${base}/paused1.bin`),
    q1: add(`${base}/q1.bin`, "--queue", q),
  };
  dm(["pause", ids.paused1]);
  await until(
    "run1 to receive bytes",
    () => Number(job(ids.run1)?.downloaded_bytes ?? 0) > 2 * MIB,
    120_000,
  );
  return { ids, pausedBefore: job(ids.paused1) };
}
async function checkWorkContinues(w) {
  slow = false;
  const done = await until(
    "run1 to finish",
    () => (job(w.ids.run1)?.phase === "completed" ? job(w.ids.run1) : null),
    240_000,
  ).catch(() => job(w.ids.run1));
  const file =
    done?.display_name && fs.existsSync(path.join(downloads, done.display_name))
      ? fs.readFileSync(path.join(downloads, done.display_name))
      : Buffer.alloc(0);
  const p = job(w.ids.paused1);
  const q = job(w.ids.q1);
  return {
    ok:
      done?.phase === "completed" &&
      sha256(file) === sha256(FILES.run1) &&
      p?.desired_state === "paused" &&
      p?.phase !== "failed" &&
      q?.phase !== "completed",
    run1: { phase: done?.phase, sha256_ok: sha256(file) === sha256(FILES.run1) },
    paused1: { desired: p?.desired_state, phase: p?.phase, error: p?.error?.kind ?? null },
    q1: { phase: q?.phase },
  };
}
function dataKept() {
  const db = path.join(dataDir, "downova.sqlite3");
  return { database_kept: fs.existsSync(db), downloads_kept: fs.readdirSync(downloads).length };
}

// ------------------------------------------------------------------ Windows
const regQuery = (key, value) => {
  const r = spawnSync("reg", ["query", key, ...(value ? ["/v", value] : ["/ve"])], {
    encoding: "utf8",
  });
  return r.status === 0 ? r.stdout : null;
};
const UNINSTALL = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Downova";
const CHROME_HOST = "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.downova.host";
const UNRELATED = "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.example.test";

async function windowsNsis() {
  const A = pkg("a", /-setup\.exe$/);
  const B = pkg("b", /-setup\.exe$/);
  results.nsis = { a: A, b: B };
  if (regQuery(UNINSTALL, "DisplayName") || fs.existsSync(dataDir)) {
    record("N0 clean account", "failed", {
      reason: "Downova is already installed or has data here",
    });
    return;
  }
  const unrelatedManifest = path.join(os.tmpdir(), "com.example.test.json");
  fs.writeFileSync(unrelatedManifest, JSON.stringify({ name: "com.example.test" }));
  spawnSync("reg", ["add", UNRELATED, "/ve", "/t", "REG_SZ", "/d", unrelatedManifest, "/f"]);
  const a = spawnSync(A.file, ["/S"], { timeout: 300_000 });
  const instDir = (regQuery(UNINSTALL, "InstallLocation") ?? "")
    .match(/REG_SZ\s+"?([^"\r\n]+)"?/)?.[1]
    ?.trim();
  binDir = instDir;
  const hostA = regQuery(CHROME_HOST);
  record(
    "N1 NSIS install A (silent, per user), versions agree, browser registration",
    a.status === 0 &&
      instDir &&
      versionOf("dm-service") === pair.a.version &&
      versionOf("dm") === pair.a.version &&
      hostA?.includes(instDir)
      ? "passed"
      : "failed",
    {
      exit: a.status,
      install_dir: instDir,
      data_dir_is_install_dir:
        instDir && path.resolve(instDir).toLowerCase() === path.resolve(dataDir).toLowerCase(),
      versions: {
        service: versionOf("dm-service"),
        cli: versionOf("dm"),
        host: versionOf("dm-browser-host"),
      },
      chrome_registration_in_install_dir: Boolean(hostA?.includes(instDir)),
    },
  );
  const pidA = startService();
  await until("A's service", () => dm(["hello"]).json?.data, 60_000);
  const w = await startWork("nsis");
  record("N2 work in progress under A (running, manually paused, stopped queue)", "passed", {
    phases: Object.fromEntries(Object.entries(w.ids).map(([k, id]) => [k, job(id)?.phase])),
  });
  const b = spawnSync(B.file, ["/S"], { timeout: 300_000 });
  await sleep(2000);
  const oldGone = !alive(pidA);
  record(
    "N3 NSIS upgrade to B while the service runs: the installer stopped only its own service",
    b.status === 0 && oldGone && versionOf("dm-service") === pair.b.version ? "passed" : "failed",
    {
      exit: b.status,
      old_service_stopped: oldGone,
      versions: {
        service: versionOf("dm-service"),
        cli: versionOf("dm"),
        host: versionOf("dm-browser-host"),
      },
    },
  );
  startService();
  await until(
    "B's service",
    () => dm(["hello"]).json?.data?.service_version === pair.b.version,
    60_000,
  ).catch(() => null);
  const cont = await checkWorkContinues(w);
  record(
    "N4 B continued the work (SHA-256), the manual pause and the stopped queue kept",
    cont.ok ? "passed" : "failed",
    cont,
  );
  slow = true;
  const extra = dm(["add", `${base}/run1.bin?again=1`]).json?.data?.job_id;
  if (extra)
    await until(
      "a running transfer for the uninstall",
      () => Number(job(extra)?.downloaded_bytes ?? 0) > 0,
      60_000,
    ).catch(() => null);
  const u = spawnSync(path.join(instDir, "uninstall.exe"), ["/S"], { timeout: 300_000 });
  await until("the uninstall to finish", () => !regQuery(UNINSTALL, "DisplayName"), 180_000).catch(
    () => null,
  );
  await sleep(3000);
  const left = processesIn(instDir);
  record(
    "N5 NSIS uninstall while the service runs: no owned process, data and downloads kept, only Downova's registration removed",
    u.status === 0 &&
      left.length === 0 &&
      dataKept().database_kept &&
      dataKept().downloads_kept > 0 &&
      !regQuery(CHROME_HOST) &&
      Boolean(regQuery(UNRELATED)) &&
      !regQuery(UNINSTALL, "DisplayName")
      ? "passed"
      : "failed",
    {
      exit: u.status,
      owned_processes_left: left,
      ...dataKept(),
      downova_registration_removed: !regQuery(CHROME_HOST),
      unrelated_registration_kept: Boolean(regQuery(UNRELATED)),
      install_dir_left: fs.existsSync(instDir) ? fs.readdirSync(instDir).sort() : null,
    },
  );
  const r = spawnSync(B.file, ["/S"], { timeout: 300_000 });
  startService();
  const back = await until(
    "the reinstalled service",
    () => dm(["list"]).json?.data?.items,
    60_000,
  ).catch(() => null);
  record(
    "N6 reinstall: the retained download list is back",
    r.status === 0 && (back ?? []).some((j) => j.job_id === w.ids.run1) ? "passed" : "failed",
    {
      exit: r.status,
      jobs: (back ?? []).length,
    },
  );
  dm(["shutdown"]);
  spawnSync(path.join(instDir, "uninstall.exe"), ["/S"], { timeout: 300_000 });
  await until("cleanup uninstall", () => !regQuery(UNINSTALL, "DisplayName"), 180_000).catch(
    () => null,
  );
  spawnSync("reg", ["delete", UNRELATED, "/f"]);
}

/**
 * Where the MSI put the service: the registry's Uninstall entry for Downova (machine-wide, both
 * views), then a shallow search of the program folders. Matched by file, never by process name.
 */
function msiInstallDir() {
  const ps = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'Downova*' } | Select-Object DisplayName,DisplayVersion,InstallLocation,InstallSource | ConvertTo-Json -Compress",
    ],
    { encoding: "utf8" },
  );
  let registry = [];
  try {
    registry = [].concat(JSON.parse(ps.stdout || "[]"));
  } catch {
    /* none */
  }
  const has = (d) => d && fs.existsSync(path.join(d, "dm-service.exe"));
  for (const e of registry)
    if (has(e.InstallLocation)) return { dir: e.InstallLocation, by: "registry", registry };
  const roots = [
    process.env.ProgramFiles,
    process.env["ProgramFiles(x86)"],
    path.join(process.env.LOCALAPPDATA, "Programs"),
  ].filter(Boolean);
  for (const r of roots) {
    let names = [];
    try {
      names = fs.readdirSync(r);
    } catch {
      continue;
    }
    for (const n of names) {
      const d = path.join(r, n);
      if (has(d)) return { dir: d, by: "search", registry };
      try {
        for (const m of fs.readdirSync(d))
          if (has(path.join(d, m))) return { dir: path.join(d, m), by: "search", registry };
      } catch {
        /* not a folder */
      }
    }
  }
  return { dir: null, by: null, registry };
}
/** The MSI log lines that say where and whether files went (UTF-16 log). */
function msiLogLines(file) {
  try {
    const raw = fs.readFileSync(file);
    const text = raw[1] === 0 ? raw.toString("utf16le") : raw.toString("utf8");
    return text
      .split(/\r?\n/)
      .filter((l) =>
        /INSTALLDIR|APPLICATIONFOLDER|TARGETDIR|Return value 3|Product: |dm-service/i.test(l),
      )
      .slice(-12)
      .map((l) => l.slice(0, 160));
  } catch {
    return null;
  }
}

async function windowsMsi() {
  const A = pkg("a", /\.msi$/);
  const B = pkg("b", /\.msi$/);
  results.msi = { a: A, b: B };
  fs.rmSync(dataDir, { recursive: true, force: true }); // owned by the previous rows on this disposable runner
  const log = (n) => path.join(outDir, `msi-${n}.log`);
  const a = spawnSync("msiexec", ["/i", A.file, "/qn", "/l*v", log("install-a")], {
    timeout: 600_000,
  });
  const found = msiInstallDir();
  binDir = found.dir;
  record(
    "M1 MSI install A (silent; folder from the Uninstall entry), versions agree",
    a.status === 0 && binDir && versionOf("dm-service") === pair.a.version ? "passed" : "failed",
    {
      exit: a.status,
      install_dir: binDir,
      found_by: found.by,
      versions: binDir ? { service: versionOf("dm-service"), cli: versionOf("dm") } : null,
      ...(binDir ? {} : { registry: found.registry, msi_log: msiLogLines(log("install-a")) }),
    },
  );
  if (!binDir) return;
  const pidA = startService();
  await until("A's service", () => dm(["hello"]).json?.data, 60_000);
  const w = await startWork("msi");
  const b = spawnSync("msiexec", ["/i", B.file, "/qn", "/norestart", "/l*v", log("upgrade-b")], {
    timeout: 600_000,
  });
  record(
    "M2 MSI upgrade to B with the service running (MSI has no Downova hooks; exit code recorded)",
    [0, 3010].includes(b.status) ? "passed" : "failed",
    {
      exit: b.status,
      reboot_required: b.status === 3010,
      old_service_still_running: alive(pidA),
      versions: { service: versionOf("dm-service"), cli: versionOf("dm") },
    },
  );
  dm(["shutdown"]);
  await until("the old service to stop", () => !alive(pidA), 60_000).catch(() => null);
  startService();
  const cont = await checkWorkContinues(w);
  record(
    "M3 after the MSI upgrade B continues the work, pause and stopped queue kept",
    cont.ok && versionOf("dm-service") === pair.b.version ? "passed" : "failed",
    cont,
  );
  dm(["shutdown"]);
  await sleep(3000);
  const x = spawnSync("msiexec", ["/x", B.file, "/qn", "/l*v", log("uninstall")], {
    timeout: 600_000,
  });
  record(
    "M4 MSI uninstall: no owned process, data and downloads kept",
    x.status === 0 && processesIn(binDir).length === 0 && dataKept().database_kept
      ? "passed"
      : "failed",
    {
      exit: x.status,
      owned_processes_left: processesIn(binDir),
      ...dataKept(),
      binaries_left: fs.existsSync(path.join(binDir, "dm-service.exe")),
    },
  );
}

// ------------------------------------------------------------------ Linux
async function linuxDeb() {
  const A = pkg("a", /\.deb$/);
  const B = pkg("b", /\.deb$/);
  results.deb = { a: A, b: B };
  if (spawnSync("dpkg", ["-s", "downova"]).status === 0 || fs.existsSync(dataDir)) {
    record("D0 clean machine", "failed", {
      reason: "Downova is already installed or has data here",
    });
    return;
  }
  const install = (f) => {
    let r = spawnSync("sudo", ["-n", "dpkg", "-i", f], { encoding: "utf8" });
    if (r.status !== 0) {
      spawnSync("sudo", ["-n", "apt-get", "install", "-f", "-y"], { stdio: "ignore" });
      r = spawnSync("sudo", ["-n", "dpkg", "-i", f], { encoding: "utf8" });
    }
    return r.status;
  };
  const pkgName = execFileSync("dpkg-deb", ["-f", A.file, "Package"], { encoding: "utf8" }).trim();
  const a = install(A.file);
  binDir = "/usr/bin";
  record(
    "D1 .deb install A (dpkg), versions agree",
    a === 0 && versionOf("dm-service") === pair.a.version ? "passed" : "failed",
    {
      exit: a,
      package: pkgName,
      versions: {
        service: versionOf("dm-service"),
        cli: versionOf("dm"),
        host: versionOf("dm-browser-host"),
      },
    },
  );
  const pidA = startService();
  await until("A's service", () => dm(["hello"]).json?.data, 60_000);
  const w = await startWork("deb");
  const b = install(B.file);
  record(
    "D2 .deb upgrade to B with dpkg while the service runs (no package hooks; old process recorded)",
    b === 0 && versionOf("dm-service") === pair.b.version ? "passed" : "failed",
    {
      exit: b,
      old_service_still_running: alive(pidA),
      hello_version_while_running: dm(["hello"]).json?.data?.service_version ?? null,
    },
  );
  dm(["shutdown"]);
  await until("the old service to stop", () => !alive(pidA), 60_000).catch(() => null);
  startService();
  const cont = await checkWorkContinues(w);
  record(
    "D3 after the upgrade B continues the work, pause and stopped queue kept",
    cont.ok ? "passed" : "failed",
    cont,
  );
  dm(["shutdown"]);
  await sleep(2000);
  const r = spawnSync("sudo", ["-n", "dpkg", "-r", pkgName]).status;
  record(
    "D4 .deb remove: binaries gone, data and downloads kept, no owned process",
    r === 0 && !fs.existsSync("/usr/bin/dm-service") && dataKept().database_kept
      ? "passed"
      : "failed",
    {
      exit: r,
      ...dataKept(),
      service_binary_left: fs.existsSync("/usr/bin/dm-service"),
    },
  );
}

// ------------------------------------------------------------------ macOS
async function macApp() {
  const A = pkg("a", /\.dmg$/);
  const B = pkg("b", /\.dmg$/);
  results.app = { a: A, b: B };
  const target = "/Applications/Downova.app";
  if (fs.existsSync(target) || fs.existsSync(dataDir)) {
    record("A0 clean machine", "failed", {
      reason: "Downova is already installed or has data here",
    });
    return;
  }
  const copyApp = (dmg) => {
    const mnt = fs.mkdtempSync(path.join(os.tmpdir(), "downova-dmg-"));
    execFileSync("hdiutil", ["attach", "-nobrowse", "-readonly", "-mountpoint", mnt, dmg]);
    const app = fs.readdirSync(mnt).find((n) => n.endsWith(".app"));
    fs.rmSync(target, { recursive: true, force: true });
    execFileSync("ditto", [path.join(mnt, app), target]);
    execFileSync("hdiutil", ["detach", mnt]);
  };
  copyApp(A.file);
  binDir = path.join(target, "Contents/MacOS");
  record(
    "A1 .app install A (copied from the .dmg), versions agree",
    versionOf("dm-service") === pair.a.version ? "passed" : "failed",
    {
      versions: { service: versionOf("dm-service"), cli: versionOf("dm") },
    },
  );
  const pidA = startService();
  await until("A's service", () => dm(["hello"]).json?.data, 60_000);
  const w = await startWork("app");
  const shutdownA = dm(["shutdown"]);
  const oldStopped = await until("the old service to stop", () => !alive(pidA), 60_000)
    .then(() => true)
    .catch(() => false);
  copyApp(B.file);
  const pidB = startService();
  const helloB = await until(
    "B's service",
    () => (helloState().ok ? helloState() : null),
    90_000,
  ).catch(() => helloState());
  const cont = await checkWorkContinues(w);
  record(
    "A2 replace with B (service stopped first): B continues the work, pause and stopped queue kept",
    cont.ok && versionOf("dm-service") === pair.b.version ? "passed" : "failed",
    {
      ...cont,
      shutdown_a: {
        code: shutdownA.code,
        error: shutdownA.json?.error ?? shutdownA.stderr ?? null,
      },
      old_service_stopped: oldStopped,
      b_service_alive: alive(pidB),
      b_hello: helloB,
      b_service_log_tail: cont.ok ? undefined : serviceLogTail(),
    },
  );
  const shutdownB = dm(["shutdown"]);
  await until("B's service to stop", () => !alive(pidB), 30_000).catch(() => null);
  fs.rmSync(target, { recursive: true, force: true });
  const left = processesIn(target);
  record(
    "A3 remove the app: data and downloads kept, no owned process",
    !fs.existsSync(target) && dataKept().database_kept && left.length === 0 ? "passed" : "failed",
    {
      ...dataKept(),
      owned_processes_left: left,
      shutdown_b: {
        code: shutdownB.code,
        error: shutdownB.json?.error ?? shutdownB.stderr ?? null,
      },
    },
  );
}

try {
  if (platform === "win32") {
    await windowsNsis();
    await windowsMsi();
  } else if (platform === "linux") await linuxDeb();
  else if (platform === "darwin") await macApp();
} catch (e) {
  record("run", "failed", { error: String(e?.stack ?? e).slice(0, 1500) });
} finally {
  server.close();
  for (const pid of owned)
    if (alive(pid))
      try {
        process.kill(pid);
      } catch {
        /* gone */
      }
  finish();
}
