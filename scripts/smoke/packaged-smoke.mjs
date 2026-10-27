#!/usr/bin/env node
// B0.5 packaged-app smoke test (docs/reports/b0.5-packaging-smoke.md).
//
//   node scripts/smoke/packaged-smoke.mjs --package-dir dist-packages/<label> [--out smoke-results/<label>]
//
// Installs (Windows NSIS per-user, Linux .deb with sudo or unpacks it without) or unpacks
// (macOS .dmg) the package produced by scripts/package.mjs, then uses ONLY the binaries inside it,
// in isolated temporary data/download directories, with no aria2 reachable:
//   A. packaged dm-service alone: `dm hello` reports backend http; `dm list`; a controlled fixture
//      download completes with the expected SHA-256.
//   B. packaged desktop (built with the test-only `smoke` feature): its window loads and its own
//      service handshake starts the packaged dm-service (no service was running).
//   C. closing the desktop during a transfer leaves the service and the download running.
//   E. forcibly terminating a desktop that started the service (taskkill /F, SIGKILL) during a
//      transfer leaves the service running and the file correct (B1.7).
//   A4/A5. the packaged dm-browser-host, started as Chrome would (origin argument, framed stdin),
//      hands a link to that service; the same idempotency key returns the same job; a caller
//      that is not on the allowlist is refused.
//   D. uninstall (where an installer was used).
// Results go to <out>/results.json and <out>/summary.md, plus logs and screenshots.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

const argv = process.argv.slice(2);
const arg = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const packageDir = path.resolve(arg("--package-dir") ?? "");
if (!fs.existsSync(packageDir)) throw new Error(`--package-dir ${packageDir} not found`);
const label = path.basename(packageDir);
const outDir = path.resolve(arg("--out") ?? path.join("smoke-results", label));
fs.mkdirSync(outDir, { recursive: true });
const platform = process.platform;
const exe = platform === "win32" ? ".exe" : "";
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-smoke-"));

const results = {
  label,
  platform,
  arch: process.arch,
  started_at: new Date().toISOString(),
  steps: [],
};
const logLines = [];
const log = (msg) => {
  const line = `[${new Date().toISOString()}] ${msg}`;
  console.log(line);
  logLines.push(line);
};
function record(step, status, detail = {}) {
  results.steps.push({ step, status, ...detail });
  log(`${status.toUpperCase().padEnd(11)} ${step} ${JSON.stringify(detail)}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

// ------------------------------------------------------------------ controlled fixture server
// Deterministic payloads with a strong ETag, Content-Length and Range support; `?slow=<ms>`
// delays every 64 KiB so a transfer is still running when the UI is closed.
function payload(len, seed) {
  const out = Buffer.alloc(len);
  let s = seed >>> 0;
  for (let i = 0; i < len; i++) {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    out[i] = s & 0xff;
  }
  return out;
}
const bodies = {
  "small.bin": payload(3 * 1024 * 1024, 7),
  "slow.bin": payload(6 * 1024 * 1024, 11),
};
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://x");
  const body = bodies[url.pathname.slice(1)];
  if (!body) return res.writeHead(404).end();
  const etag = `"${sha256(body).slice(0, 16)}"`;
  let start = 0;
  let end = body.length - 1;
  let status = 200;
  const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
  const ifRange = req.headers["if-range"];
  if (range && (!ifRange || ifRange === etag)) {
    start = Number(range[1]);
    end = range[2] ? Number(range[2]) : body.length - 1;
    status = 206;
  }
  const headers = { "Content-Length": end - start + 1, ETag: etag, "Accept-Ranges": "bytes" };
  if (status === 206) headers["Content-Range"] = `bytes ${start}-${end}/${body.length}`;
  res.writeHead(status, headers);
  const slow = Number(url.searchParams.get("slow") ?? 0);
  for (let i = start; i <= end; i += 65536) {
    if (res.destroyed) return;
    res.write(body.subarray(i, Math.min(i + 65536, end + 1)));
    if (slow) await sleep(slow);
  }
  res.end();
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

// ------------------------------------------------------------------ install / unpack
const pkgFiles = fs.readdirSync(packageDir);
const find = (re) => pkgFiles.find((f) => re.test(f));
let binDir;
let desktopExe;
let uninstall = null;
const installDir = path.join(work, "install");

function installWindows() {
  const setup = find(/-setup\.exe$/i);
  if (!setup) throw new Error("no NSIS installer in package dir");
  const r = spawnSync(path.join(packageDir, setup), ["/S", `/D=${installDir}`], {
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error(`NSIS installer exited ${r.status}`);
  binDir = installDir;
  uninstall = () => {
    const u = path.join(installDir, "uninstall.exe");
    const r2 = spawnSync(u, ["/S"], { stdio: "inherit" });
    return r2.status === 0;
  };
  record("install (NSIS, per-user, silent)", "passed", { installer: setup, dir: installDir });
  const msi = find(/\.msi$/i);
  if (msi) {
    const target = path.join(work, "msi-extract");
    const r3 = spawnSync(
      "msiexec",
      ["/a", path.join(packageDir, msi), "/qn", `TARGETDIR=${target}`],
      { stdio: "inherit" },
    );
    const hasService = r3.status === 0 && findFile(target, `dm-service${exe}`);
    record("MSI administrative extract (no system install)", hasService ? "passed" : "failed", {
      msi,
      exit: r3.status,
      contains_dm_service: Boolean(hasService),
    });
  }
}

function installMac() {
  const dmg = find(/\.dmg$/);
  if (!dmg) throw new Error("no .dmg in package dir");
  const mnt = path.join(work, "mnt");
  fs.mkdirSync(mnt);
  execFileSync("hdiutil", [
    "attach",
    "-nobrowse",
    "-readonly",
    "-mountpoint",
    mnt,
    path.join(packageDir, dmg),
  ]);
  const app = fs.readdirSync(mnt).find((n) => n.endsWith(".app"));
  fs.mkdirSync(installDir, { recursive: true });
  execFileSync("ditto", [path.join(mnt, app), path.join(installDir, app)]);
  execFileSync("hdiutil", ["detach", mnt]);
  binDir = path.join(installDir, app, "Contents", "MacOS");
  const sig = spawnSync("codesign", ["-dv", path.join(installDir, app)], { encoding: "utf8" });
  record("unpack .dmg → .app", "passed", {
    dmg,
    app,
    codesign: (sig.stderr || "").split("\n").filter((l) => /Signature|Identifier/.test(l)),
  });
  uninstall = () => {
    fs.rmSync(path.join(installDir, app), { recursive: true, force: true });
    return !fs.existsSync(path.join(installDir, app));
  };
}

function installLinux() {
  const deb = find(/\.deb$/);
  if (!deb) throw new Error("no .deb in package dir");
  const pkgName = execFileSync("dpkg-deb", ["-f", path.join(packageDir, deb), "Package"], {
    encoding: "utf8",
  }).trim();
  const canSudo = spawnSync("sudo", ["-n", "true"]).status === 0;
  if (canSudo) {
    const r = spawnSync("sudo", ["-n", "dpkg", "-i", path.join(packageDir, deb)], {
      stdio: "inherit",
    });
    if (r.status !== 0) {
      spawnSync("sudo", ["-n", "apt-get", "install", "-f", "-y"], { stdio: "inherit" });
    }
    binDir = "/usr/bin";
    uninstall = () =>
      spawnSync("sudo", ["-n", "dpkg", "-r", pkgName], { stdio: "inherit" }).status === 0;
    record("install .deb (dpkg -i)", "passed", { deb, package: pkgName });
  } else {
    execFileSync("dpkg-deb", ["-x", path.join(packageDir, deb), installDir]);
    binDir = path.join(installDir, "usr", "bin");
    record("unpack .deb (dpkg-deb -x; no sudo)", "passed", { deb, package: pkgName });
  }
  const appImage = find(/\.AppImage$/);
  if (appImage) {
    const dir = path.join(work, "appimage");
    fs.mkdirSync(dir);
    fs.copyFileSync(path.join(packageDir, appImage), path.join(dir, appImage));
    fs.chmodSync(path.join(dir, appImage), 0o755);
    const r = spawnSync(path.join(dir, appImage), ["--appimage-extract"], {
      cwd: dir,
      encoding: "utf8",
    });
    const sidecar = findFile(path.join(dir, "squashfs-root"), "dm-service");
    record(
      "AppImage extract contains the service sidecar",
      r.status === 0 && sidecar ? "passed" : "failed",
      { appImage, sidecar: sidecar ? path.relative(dir, sidecar) : null },
    );
  }
}

function findFile(dir, name) {
  if (!fs.existsSync(dir)) return null;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name === name) return p;
  }
  return null;
}

// ------------------------------------------------------------------ helpers using packaged binaries
const env = (home, dl) => ({
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  DM_ARIA2_PATH: path.join(work, "no-such-aria2c"),
});
// One run of the packaged native host with framed messages (4-byte native-endian length).
const DEV_ORIGIN = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";
function nativeHost(exePath, home, dl, origin, messages) {
  const frames = messages.map((m) => {
    const body = Buffer.from(JSON.stringify(m));
    const len = Buffer.alloc(4);
    if (os.endianness() === "LE") len.writeUInt32LE(body.length);
    else len.writeUInt32BE(body.length);
    return Buffer.concat([len, body]);
  });
  const r = spawnSync(exePath, [origin], {
    env: env(home, dl),
    cwd: work,
    input: Buffer.concat(frames),
    timeout: 60_000,
  });
  const out = [];
  let buf = r.stdout ?? Buffer.alloc(0);
  while (buf.length >= 4) {
    const n = os.endianness() === "LE" ? buf.readUInt32LE(0) : buf.readUInt32BE(0);
    out.push(JSON.parse(buf.subarray(4, 4 + n).toString("utf8")));
    buf = buf.subarray(4 + n);
  }
  return { code: r.status, replies: out, stderr: String(r.stderr ?? "").slice(-2000) };
}
function dm(home, dl, args) {
  const r = spawnSync(path.join(binDir, `dm${exe}`), ["--data-dir", home, "--json", ...args], {
    env: env(home, dl),
    cwd: work,
    encoding: "utf8",
    timeout: 60_000,
  });
  let json = null;
  try {
    json = JSON.parse(r.stdout.trim());
  } catch {
    json = null; // non-JSON output is reported through `code` and `stderr`
  }
  return { code: r.status, json, stderr: r.stderr };
}
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}
async function download(home, dl, url, expected) {
  const add = dm(home, dl, ["add", url]);
  if (add.code !== 0) throw new Error(`dm add exited ${add.code}: ${add.stderr}`);
  const job = add.json.data.job_id;
  const done = await until(
    "download to finish",
    () => {
      const g = dm(home, dl, ["get", job]);
      return ["completed", "failed", "cancelled"].includes(g.json?.data?.phase)
        ? g.json.data
        : null;
    },
    120_000,
  );
  const file = fs.existsSync(path.join(dl, done.display_name))
    ? fs.readFileSync(path.join(dl, done.display_name))
    : Buffer.alloc(0);
  return {
    job,
    phase: done.phase,
    sha256: sha256(file),
    expected: sha256(expected),
    bytes: file.length,
  };
}
function killTree(pid) {
  if (!pid) return;
  if (platform === "win32") spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"]);
  else spawnSync("kill", ["-9", String(pid)]);
}
function servicePid(home) {
  try {
    return JSON.parse(fs.readFileSync(path.join(home, "run", "endpoint.json"), "utf8")).pid;
  } catch {
    return null;
  }
}
function alive(pid) {
  if (!pid) return false;
  if (platform === "win32") {
    const r = spawnSync("tasklist", ["/FI", `PID eq ${pid}`, "/NH"], { encoding: "utf8" });
    return r.stdout.includes(String(pid));
  }
  return spawnSync("kill", ["-0", String(pid)]).status === 0;
}
// Screenshots never capture the whole desktop (Windows: the window's visible frame from
// DWMWA_EXTENDED_FRAME_BOUNDS, without the invisible resize border) on a developer machine (it may show unrelated,
// private windows): on Windows only the Downova window's rectangle is captured; on macOS and
// Linux a full-screen capture is taken only on CI runners (`CI` set), where the screen holds
// nothing but the test.
function screenshot(name, pid) {
  const file = path.join(outDir, `${name}.png`);
  let r;
  if (platform === "win32") {
    const ps = [
      "Add-Type -AssemblyName System.Drawing;",
      "Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"dwmapi.dll\")] public static extern int DwmGetWindowAttribute(System.IntPtr h, int a, out RECT r, int n); public struct RECT { public int L; public int T; public int R; public int B; }';",
      `$h=(Get-Process -Id ${pid ?? 0} -ErrorAction Stop).MainWindowHandle; if ($h -eq 0) { exit 3 };`,
      "$r=New-Object W.U+RECT; [void][W.U]::DwmGetWindowAttribute($h,9,[ref]$r,16); $w=$r.R-$r.L; $hh=$r.B-$r.T; if ($w -le 0 -or $hh -le 0) { exit 4 };",
      "$bmp=New-Object System.Drawing.Bitmap $w,$hh; $g=[System.Drawing.Graphics]::FromImage($bmp);",
      `$g.CopyFromScreen($r.L,$r.T,0,0,$bmp.Size); $bmp.Save('${file}')`,
    ].join(" ");
    r = spawnSync("powershell", ["-NoProfile", "-Command", ps]);
  } else if (!process.env.CI) {
    record(`screenshot ${name}`, "unavailable", {
      reason: "full-screen capture only on CI runners",
    });
    return;
  } else if (platform === "darwin") {
    r = spawnSync("screencapture", ["-x", file]);
  } else {
    r = spawnSync("import", ["-window", "root", file]);
  }
  const ok = r.status === 0 && fs.existsSync(file);
  record(`screenshot ${name}`, ok ? "passed" : "unavailable", {
    file: ok ? path.basename(file) : null,
    scope: platform === "win32" ? "Downova window only" : "full screen (CI)",
  });
}

// ------------------------------------------------------------------ run
let exitCode = 0;
const spawned = [];
try {
  log(`package dir ${packageDir}; work dir ${work}; fixture ${base}`);
  if (platform === "win32") installWindows();
  else if (platform === "darwin") installMac();
  else installLinux();

  const names = fs.readdirSync(binDir);
  // Tauri names the main executable after the Cargo binary (dm-desktop) or the product name,
  // depending on the bundle; accept either, never the sidecars.
  desktopExe = ["dm-desktop", "downova", "Downova"]
    .map((n) => path.join(binDir, `${n}${exe}`))
    .find((p) => fs.existsSync(p));
  const bins = {
    service: path.join(binDir, `dm-service${exe}`),
    cli: path.join(binDir, `dm${exe}`),
    host: path.join(binDir, `dm-browser-host${exe}`),
    desktop: desktopExe,
  };
  for (const [k, p] of Object.entries(bins)) {
    if (!p || !fs.existsSync(p))
      throw new Error(`packaged ${k} binary not found in ${binDir}: ${names.join(", ")}`);
  }
  record("packaged binaries present", "passed", {
    bin_dir: binDir,
    desktop: path.basename(desktopExe),
  });
  const aria2OnPath = spawnSync(platform === "win32" ? "where" : "which", ["aria2c"]).status === 0;
  record("aria2c not reachable (DM_ARIA2_PATH points nowhere)", aria2OnPath ? "note" : "passed", {
    aria2c_on_path: aria2OnPath,
  });

  // A. service alone
  const homeA = path.join(work, "a-data");
  const dlA = path.join(work, "a-downloads");
  fs.mkdirSync(dlA, { recursive: true });
  const svc = spawn(bins.service, ["--data-dir", homeA, "--download-dir", dlA], {
    env: env(homeA, dlA),
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(svc.pid);
  const hello = await until(
    "packaged service hello",
    () => {
      const h = dm(homeA, dlA, ["hello"]);
      return h.code === 0 ? h.json.data : null;
    },
    30_000,
  );
  record(
    "A1 packaged service: dm hello reports backend http",
    hello.backend.id === "http" ? "passed" : "failed",
    {
      backend: hello.backend.id,
      service_version: hello.service_version,
    },
  );
  const list = dm(homeA, dlA, ["list"]);
  record("A2 dm list", list.code === 0 ? "passed" : "failed", {
    items: list.json?.data?.items?.length,
  });
  const a3 = await download(homeA, dlA, `${base}/small.bin`, bodies["small.bin"]);
  record(
    "A3 fixture download, SHA-256 verified",
    a3.phase === "completed" && a3.sha256 === a3.expected ? "passed" : "failed",
    a3,
  );
  const hostAdd = (n) => ({
    v: 1,
    request_id: `req_smoke${n}`,
    op: "add",
    url: `${base}/small.bin?via=host`,
    idempotency_key: "ext_smoke_0001",
  });
  const viaHost = nativeHost(bins.host, homeA, dlA, DEV_ORIGIN, [hostAdd(1), hostAdd(2)]);
  const [first, again] = viaHost.replies;
  record(
    "A4 packaged native host: add accepted; same key returns the same job",
    viaHost.code === 0 &&
      first?.outcome === "accepted" &&
      again?.outcome === "accepted" &&
      first.job?.job_id === again.job?.job_id
      ? "passed"
      : "failed",
    {
      code: viaHost.code,
      outcomes: viaHost.replies.map((r) => r.outcome),
      job_ids: viaHost.replies.map((r) => r.job?.job_id ?? null),
      stderr: viaHost.code === 0 ? undefined : viaHost.stderr,
    },
  );
  const foreign = nativeHost(
    bins.host,
    homeA,
    dlA,
    "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
    [hostAdd(3)],
  );
  record(
    "A5 packaged native host refuses an extension not on the allowlist",
    foreign.code === 1 && foreign.replies[0]?.outcome === "rejected" ? "passed" : "failed",
    { code: foreign.code, outcome: foreign.replies[0]?.outcome },
  );
  killTree(svc.pid);

  // B. desktop starts the service itself
  const homeB = path.join(work, "b-data");
  const dlB = path.join(work, "b-downloads");
  fs.mkdirSync(dlB, { recursive: true });
  const smokeFile = path.join(work, "smoke.json");
  const desktop = spawn(desktopExe, [], {
    env: { ...env(homeB, dlB), DOWNOVA_SMOKE_FILE: smokeFile },
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(desktop.pid);
  let desktopExited = false;
  desktop.on("exit", () => (desktopExited = true));
  const report = await until(
    "desktop smoke report",
    () => (fs.existsSync(smokeFile) ? JSON.parse(fs.readFileSync(smokeFile, "utf8")) : null),
    90_000,
  );
  fs.writeFileSync(path.join(outDir, "desktop-smoke.json"), JSON.stringify(report, null, 2));
  record("B1 desktop window loaded", report.window?.title === "Downova" ? "passed" : "failed", {
    window: report.window,
  });
  record(
    "B2 desktop handshake started the packaged service",
    report.service?.status === "connected" && report.service?.hello?.backend?.id === "http"
      ? "passed"
      : "failed",
    {
      service: report.service?.status,
      backend: report.service?.hello?.backend?.id,
      sidecar: report.sidecar,
    },
  );
  const svcB = servicePid(homeB);
  spawned.push(svcB);
  const helloB = dm(homeB, dlB, ["hello"]);
  record(
    "B3 dm hello against the desktop-started service",
    helloB.code === 0 && helloB.json.data.backend.id === "http" ? "passed" : "failed",
    { service_pid: svcB },
  );
  await sleep(1500);
  screenshot("desktop-window", desktop.pid);

  // C. close the UI during a transfer
  const add = dm(homeB, dlB, ["add", `${base}/slow.bin?slow=40`]);
  const job = add.json?.data?.job_id;
  await until(
    "transfer to start",
    () => Number(dm(homeB, dlB, ["get", job]).json?.data?.downloaded_bytes ?? 0) > 0,
    30_000,
  );
  const closeVia =
    platform === "win32"
      ? "taskkill (WM_CLOSE)"
      : platform === "darwin"
        ? "osascript quit"
        : "SIGTERM";
  if (platform === "win32") spawnSync("taskkill", ["/PID", String(desktop.pid)]);
  else if (platform === "darwin") spawnSync("osascript", ["-e", 'quit app "Downova"']);
  else process.kill(desktop.pid, "SIGTERM");
  let graceful = true;
  try {
    await until("desktop to exit", () => desktopExited, 20_000);
  } catch {
    graceful = false;
    killTree(desktop.pid);
  }
  const midway = dm(homeB, dlB, ["get", job]).json?.data;
  record("C1 desktop closed during a transfer", graceful ? "passed" : "note", {
    via: closeVia,
    graceful,
    downloaded_bytes_at_close: midway?.downloaded_bytes,
    phase: midway?.phase,
  });
  record("C2 service still running after the UI closed", alive(svcB) ? "passed" : "failed", {
    service_pid: svcB,
  });
  const finished = await until(
    "download after UI close",
    () => {
      const g = dm(homeB, dlB, ["get", job]).json?.data;
      return g && ["completed", "failed", "cancelled"].includes(g.phase) ? g : null;
    },
    180_000,
  );
  const file = fs.readFileSync(path.join(dlB, finished.display_name));
  record(
    "C3 download completed after the UI closed, SHA-256 verified",
    finished.phase === "completed" && sha256(file) === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    {
      phase: finished.phase,
      sha256: sha256(file),
    },
  );
  killTree(svcB);

  // E. forcibly terminate the desktop that started the service, during a transfer (B1.7,
  // CORE-04). Only the desktop process is killed (Windows `taskkill /F` without /T, Unix SIGKILL):
  // the service it started is its child in the process table, yet must keep running.
  const homeE = path.join(work, "e-data");
  const dlE = path.join(work, "e-downloads");
  fs.mkdirSync(dlE, { recursive: true });
  const smokeFileE = path.join(work, "smoke-e.json");
  const desktopE = spawn(desktopExe, [], {
    env: { ...env(homeE, dlE), DOWNOVA_SMOKE_FILE: smokeFileE },
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(desktopE.pid);
  let desktopEExited = false;
  desktopE.on("exit", () => (desktopEExited = true));
  const reportE = await until(
    "second desktop smoke report",
    () => (fs.existsSync(smokeFileE) ? JSON.parse(fs.readFileSync(smokeFileE, "utf8")) : null),
    90_000,
  );
  const svcE = servicePid(homeE);
  spawned.push(svcE);
  record(
    "E1 desktop started its own service",
    reportE.service?.status === "connected" && alive(svcE) ? "passed" : "failed",
    { service_pid: svcE, desktop_pid: desktopE.pid },
  );
  const addE = dm(homeE, dlE, ["add", `${base}/slow.bin?slow=40&kill=1`]);
  const jobE = addE.json?.data?.job_id;
  await until(
    "transfer to start before the kill",
    () => Number(dm(homeE, dlE, ["get", jobE]).json?.data?.downloaded_bytes ?? 0) > 0,
    30_000,
  );
  const killVia = platform === "win32" ? "taskkill /F (no /T)" : "SIGKILL";
  if (platform === "win32") spawnSync("taskkill", ["/PID", String(desktopE.pid), "/F"]);
  else process.kill(desktopE.pid, "SIGKILL");
  await until("desktop to die", () => desktopEExited, 20_000);
  const atKill = dm(homeE, dlE, ["get", jobE]).json?.data;
  record("E2 desktop forcibly terminated during a transfer", "passed", {
    via: killVia,
    downloaded_bytes_at_kill: atKill?.downloaded_bytes,
    phase: atKill?.phase,
  });
  await sleep(1000);
  record(
    "E3 service still running after the desktop was killed",
    alive(svcE) ? "passed" : "failed",
    {
      service_pid: svcE,
    },
  );
  const finishedE = await until(
    "download after the desktop was killed",
    () => {
      const g = dm(homeE, dlE, ["get", jobE]).json?.data;
      return g && ["completed", "failed", "cancelled"].includes(g.phase) ? g : null;
    },
    180_000,
  );
  const fileE = fs.existsSync(path.join(dlE, finishedE.display_name))
    ? fs.readFileSync(path.join(dlE, finishedE.display_name))
    : Buffer.alloc(0);
  record(
    "E4 download completed after the desktop was killed, SHA-256 verified",
    finishedE.phase === "completed" && sha256(fileE) === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    { phase: finishedE.phase, sha256: sha256(fileE), error: finishedE.error ?? null },
  );
  killTree(svcE);

  // D. uninstall
  if (uninstall) {
    await sleep(1000);
    const ok = uninstall();
    if (platform === "win32") {
      // The NSIS uninstaller re-launches itself from %TEMP%; wait for the files to disappear.
      try {
        await until(
          "uninstall to remove the service binary",
          () => !fs.existsSync(bins.service),
          30_000,
        );
      } catch {
        // reported below: the binary is still present
      }
    }
    record("D uninstall", ok && !fs.existsSync(bins.service) ? "passed" : "failed", {
      service_binary_removed: !fs.existsSync(bins.service),
    });
  } else {
    record("D uninstall", "unavailable", { reason: "package was unpacked, not installed" });
  }
} catch (e) {
  record("smoke run", "failed", { error: String(e?.message ?? e) });
  screenshot("failure");
} finally {
  for (const pid of spawned) killTree(pid);
  server.close();
  results.finished_at = new Date().toISOString();
  const failed = results.steps.filter((s) => s.status === "failed");
  results.verdict = failed.length === 0 ? "passed" : "failed";
  exitCode = failed.length === 0 ? 0 : 1;
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2));
  fs.writeFileSync(path.join(outDir, "smoke.log"), logLines.join("\n") + "\n");
  const md = [
    `# Packaged-app smoke: ${label}`,
    "",
    `- platform: ${platform} ${process.arch}`,
    `- verdict: **${results.verdict}**`,
    "",
    "| Step | Status | Detail |",
    "|---|---|---|",
    ...results.steps.map((s) => {
      const { step, status, ...d } = s;
      return `| ${step} | ${status} | ${JSON.stringify(d).replace(/\|/g, "\\|").slice(0, 300)} |`;
    }),
  ].join("\n");
  fs.writeFileSync(path.join(outDir, "summary.md"), md + "\n");
  console.log(`\n${md}\n`);
  process.exit(exitCode);
}
