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
//   F. the packaged desktop UI adds a fixture through its production command path (Add URL
//      dialog → downloads_add); progress and completion are observed through downloads_list and
//      the file's SHA-256 is checked (B2.1).
//   G. closing the window (default: hide to tray) keeps the desktop and the transfer running; a
//      second launch shows the existing window and exits (B2.2).
//   H. confirmed "Exit and stop downloads" (tray path + real dialog) pauses the transfer and stops
//      the service; after a restart the job is paused until Resume completes it (B2.2).
//   I. Cancel download and Remove from list through the packaged UI; the file stays byte-exact
//      and removed jobs stay out of the list after a restart (B2.3).
//   A4/A5. the packaged dm-browser-host, started as Chrome would (origin argument, framed stdin),
//      hands a link to that service; the same idempotency key returns the same job; a caller
//      that is not on the allowlist is refused.
//   R. browser registration through the production flow (B2.5, ADR 0014), in an isolated
//      per-user location (Windows: LOCALAPPDATA for the manifests, HKCU values backed up and
//      restored; macOS/Linux: a temporary HOME):
//      R0 no registration before install; R1 the NSIS installer (Windows) and the desktop's first
//      launch (all OSes) register Chrome and Edge, owned by this installation, exact manifest,
//      path, permissions and origins; R2 desktop window and service ready; R3 the real Chrome
//      (and Edge where installed) extension hand-off against the installed host using only the
//      product registration (scripts/e2e/chrome-handoff.mjs --product-registration; the menu
//      click is simulated, everything after it is real); R4 the desktop's downloads_list shows
//      the hand-off's progress, exactly one job, SHA-256; R5 repair after a missing, damaged or
//      unreadable manifest (and a missing registry value on Windows).
//   D. uninstall (where an installer was used); D0 macOS/Linux packages have no per-user
//      uninstall hook, so the documented `dm-browser-host unregister` runs first; D2 this
//      installation's registration is gone, unrelated entries next to it survive, downloads and
//      data are kept.
// Results go to <out>/results.json and <out>/summary.md, plus logs and screenshots.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

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

// ------------------------------------------------------------------ controlled fixture server
// Deterministic payloads with a strong ETag, Content-Length and Range support; `?slow=<ms>`
// delays every 64 KiB so a transfer is still running when the UI is closed.
const bodies = {
  "small.bin": payload(3 * 1024 * 1024, 7),
  "slow.bin": payload(6 * 1024 * 1024, 11),
  "handoff.bin": payload(4 * 1024 * 1024, 13),
  "handoff-edge.bin": payload(2 * 1024 * 1024, 17),
};
const { server, base } = await startFixtureServer(bodies);

// ------------------------------------------------------------------ install / unpack
const pkgFiles = fs.readdirSync(packageDir);
const find = (re) => pkgFiles.find((f) => re.test(f));
let binDir;
let desktopExe;
let uninstall = null;
const installDir = path.join(work, "install");
// B2.5: the per-user locations registration writes to, isolated for this run.
const regHome = path.join(work, "r-userhome");
const regLocal = path.join(work, "r-localappdata");
fs.mkdirSync(regHome, { recursive: true });
fs.mkdirSync(regLocal, { recursive: true });
const regEnv =
  platform === "win32"
    ? { LOCALAPPDATA: regLocal }
    : { HOME: regHome, XDG_CONFIG_HOME: path.join(regHome, ".config") };
const HOST_KEYS =
  platform === "win32"
    ? ["Google\\Chrome", "Microsoft\\Edge"].map(
        (v) => `HKCU\\Software\\${v}\\NativeMessagingHosts\\com.downova.host`,
      )
    : [];
const UNRELATED_KEY =
  "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.example.downova_smoke_unrelated";
const regQuery = (key) => {
  const q = spawnSync("reg", ["query", key, "/ve"], { encoding: "utf8" });
  return q.status === 0 ? (/REG_SZ\s+(.+)$/m.exec(q.stdout)?.[1]?.trim() ?? "") : null;
};
// Windows: a developer's existing HKCU values are saved, removed for the run and restored.
const savedKeys = new Map();
function isolateRegistry() {
  for (const key of HOST_KEYS) {
    const v = regQuery(key);
    if (v !== null) {
      savedKeys.set(key, v);
      spawnSync("reg", ["delete", key, "/f"]);
    }
  }
}
function restoreRegistry() {
  for (const key of [...HOST_KEYS, UNRELATED_KEY]) {
    const v = regQuery(key);
    // Only values this run created (pointing into the work directory) are removed.
    if (v !== null && (v.startsWith(work) || key === UNRELATED_KEY))
      spawnSync("reg", ["delete", key, "/f"]);
  }
  for (const [key, v] of savedKeys)
    spawnSync("reg", ["add", key, "/ve", "/t", "REG_SZ", "/d", v, "/f"]);
}

function installWindows() {
  const setup = find(/-setup\.exe$/i);
  if (!setup) throw new Error("no NSIS installer in package dir");
  // The installer's hook registers for the installing user; its manifests go to the isolated
  // LOCALAPPDATA.
  const r = spawnSync(path.join(packageDir, setup), ["/S", `/D=${installDir}`], {
    stdio: "inherit",
    env: { ...process.env, ...regEnv },
  });
  if (r.status !== 0) throw new Error(`NSIS installer exited ${r.status}`);
  binDir = installDir;
  uninstall = () => {
    const u = path.join(installDir, "uninstall.exe");
    const r2 = spawnSync(u, ["/S"], { stdio: "inherit", env: { ...process.env, ...regEnv } });
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
  // Only step R exercises the desktop's first-launch browser registration (B2.5).
  DOWNOVA_SKIP_BROWSER_SETUP: "1",
});
// The packaged host's registration commands in the isolated per-user location.
function hostReg(args) {
  const r = spawnSync(path.join(binDir, `dm-browser-host${exe}`), [...args, "--json"], {
    env: { ...process.env, ...regEnv },
    cwd: work,
    encoding: "utf8",
    timeout: 60_000,
  });
  const rows = [];
  for (const line of (r.stdout ?? "").split("\n")) {
    try {
      if (line.trim()) rows.push(JSON.parse(line));
    } catch {
      // non-JSON output is reported through `stderr`
    }
  }
  return { code: r.status, rows, stderr: (r.stderr ?? "").slice(-1000) };
}
const readOr = (f) => (fs.existsSync(f) ? fs.readFileSync(f) : Buffer.alloc(0));
const BROWSERS = ["--browser", "chrome", "--browser", "edge"];
const samePath = (a, b) =>
  platform === "win32"
    ? path.resolve(a).toLowerCase() === path.resolve(b).toLowerCase()
    : path.resolve(a) === path.resolve(b);
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
// A child process awaited without blocking this process (the fixture server keeps serving).
function runChild(cmd, args, options, ms) {
  return new Promise((resolve) => {
    const child = spawn(cmd, args, options);
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      killTree(child.pid);
    }, ms);
    child.on("exit", (status) => {
      clearTimeout(timer);
      resolve({ status, timedOut });
    });
  });
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

// On GitHub Actions every row is published as one notice annotation: annotations are readable
// through the public API, job logs and artifacts are not (B2.6).
function publishRows(title, results) {
  if (process.env.GITHUB_ACTIONS !== "true") return;
  const escape = (v) => v.replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
  const counts = {};
  for (const s of results.steps) counts[s.status] = (counts[s.status] ?? 0) + 1;
  const lines = [
    `verdict ${results.verdict}; ${Object.entries(counts)
      .map(([k, n]) => `${n} ${k}`)
      .join(", ")}`,
    ...results.steps.map((s) => `${s.status.toUpperCase()} ${s.step}`),
  ];
  const level = results.verdict === "failed" ? "error" : "notice";
  console.log(
    `::${level} title=${escape(title).replace(/[,:]/g, " ")}::${escape(lines.join("\n").slice(0, 60_000))}`,
  );
}

// ------------------------------------------------------------------ run
let exitCode = 0;
const spawned = [];
try {
  log(`package dir ${packageDir}; work dir ${work}; fixture ${base}`);
  if (platform === "win32") isolateRegistry();
  record("R0 isolated start: no Downova registration in the per-user location", "passed", {
    location: platform === "win32" ? `HKCU + ${regLocal}` : regHome,
    pre_existing_hkcu_values_saved: savedKeys.size,
  });
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
  // Step C checks that closing the UI leaves the service running, so this desktop closes for
  // real (B2.2 default is hide-to-tray, covered by step G).
  fs.mkdirSync(homeB, { recursive: true });
  fs.writeFileSync(
    path.join(homeB, "desktop-settings.json"),
    JSON.stringify({ close_behaviour: "close_window" }),
  );
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

  // F. the production desktop command path (B2.1). The smoke-only driver in the desktop fills
  // the real Add URL dialog and submits it (React → ServiceClient → Tauri `downloads_add` →
  // service), then records what `downloads_list` reports until the job ends. This proves the
  // packaged bridge; the dialog's validation, selection and Stop/Resume are proven by the UI
  // tests with a fake client.
  const homeF = path.join(work, "f-data");
  const dlF = path.join(work, "f-downloads");
  fs.mkdirSync(dlF, { recursive: true });
  const bridgeFile = path.join(work, "bridge-f.json");
  const urlF = `${base}/slow.bin?slow=60&via=ui`;
  const desktopF = spawn(desktopExe, [], {
    env: {
      ...env(homeF, dlF),
      DOWNOVA_SMOKE_FILE: path.join(work, "smoke-f.json"),
      DOWNOVA_SMOKE_BRIDGE_URL: urlF,
      DOWNOVA_SMOKE_BRIDGE_FILE: bridgeFile,
    },
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(desktopF.pid);
  const bridge = await until(
    "the UI-added download to end",
    () => (fs.existsSync(bridgeFile) ? JSON.parse(fs.readFileSync(bridgeFile, "utf8")) : null),
    240_000,
  );
  fs.writeFileSync(path.join(outDir, "desktop-bridge.json"), JSON.stringify(bridge, null, 2));
  spawned.push(servicePid(homeF));
  const jobF = bridge.job;
  record(
    "F1 Add URL in the packaged desktop UI reached the service through downloads_add",
    jobF && bridge.jobs_in_list === 1 ? "passed" : "failed",
    {
      job_id: jobF?.job_id ?? null,
      jobs_in_list: bridge.jobs_in_list,
      timed_out: !!bridge.timed_out,
    },
  );
  const moving = (bridge.samples ?? []).filter(
    (s) =>
      s.phase === "transferring" &&
      BigInt(s.downloaded_bytes) > 0n &&
      s.total_bytes !== null &&
      BigInt(s.downloaded_bytes) < BigInt(s.total_bytes),
  );
  record(
    "F2 partial progress observed through downloads_list",
    moving.length > 0 ? "passed" : "failed",
    { samples: bridge.samples?.length ?? 0, partial_samples: moving.length },
  );
  const fileF =
    jobF && fs.existsSync(path.join(dlF, jobF.display_name))
      ? fs.readFileSync(path.join(dlF, jobF.display_name))
      : Buffer.alloc(0);
  record(
    "F3 the UI-added download completed, SHA-256 verified",
    jobF?.phase === "completed" &&
      sha256(fileF) === sha256(bodies["slow.bin"]) &&
      jobF.verification?.sha256 === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    { phase: jobF?.phase, sha256: sha256(fileF), verification: jobF?.verification ?? null },
  );
  const viaCli = jobF ? dm(homeF, dlF, ["get", jobF.job_id]).json?.data : null;
  record(
    "F4 the CLI sees the same job (one service, one contract)",
    viaCli?.phase === "completed" && viaCli.display_name === jobF.display_name
      ? "passed"
      : "failed",
    { phase: viaCli?.phase ?? null },
  );
  await sleep(1500);
  screenshot("desktop-live-list", desktopF.pid);
  killTree(desktopF.pid);
  // One desktop per user at a time (single instance, B2.2): G must not find F still running.
  await until("desktop F to exit", () => !alive(desktopF.pid), 20_000);
  killTree(servicePid(homeF));

  // G. close-to-tray and single instance (B2.2). Default close behaviour (hide to tray). The window
  // is closed through Tauri (the same close request as its close button); real title-bar clicks
  // and tray-menu clicks are manual checks.
  const homeG = path.join(work, "g-data");
  const dlG = path.join(work, "g-downloads");
  fs.mkdirSync(dlG, { recursive: true });
  const lifeFile = path.join(work, "lifecycle-g.jsonl");
  const closeWhen = path.join(work, "close-g.now");
  const exitWhen = path.join(work, "exit-g.now");
  const envG = {
    ...env(homeG, dlG),
    DOWNOVA_SMOKE_FILE: path.join(work, "smoke-g.json"),
    DOWNOVA_SMOKE_LIFECYCLE_FILE: lifeFile,
    DOWNOVA_SMOKE_CLOSE_WHEN: closeWhen,
    DOWNOVA_SMOKE_EXIT_WHEN: exitWhen,
  };
  const lifeEvents = () =>
    fs.existsSync(lifeFile)
      ? fs
          .readFileSync(lifeFile, "utf8")
          .split("\n")
          .filter(Boolean)
          .map((l) => JSON.parse(l))
      : [];
  const desktopG = spawn(desktopExe, [], { env: envG, cwd: work, stdio: "ignore" });
  spawned.push(desktopG.pid);
  let desktopGExited = false;
  desktopG.on("exit", () => (desktopGExited = true));
  await until("desktop G connected", () => fs.existsSync(path.join(work, "smoke-g.json")), 90_000);
  const svcG = servicePid(homeG);
  spawned.push(svcG);
  const addG = dm(homeG, dlG, ["add", `${base}/slow.bin?slow=40&step=g`]);
  const jobG = addG.json?.data?.job_id;
  await until(
    "transfer G to start",
    () => Number(dm(homeG, dlG, ["get", jobG]).json?.data?.downloaded_bytes ?? 0) > 0,
    30_000,
  );
  fs.writeFileSync(closeWhen, "");
  const closed = await until(
    "the close request",
    () => lifeEvents().find((e) => e.event === "after_close"),
    20_000,
  );
  const closeAction = lifeEvents().find((e) => e.event === "close_requested")?.detail?.action;
  record(
    "G1 closing the window hides it (or minimizes without a tray); the desktop keeps running",
    ["hidden", "minimized"].includes(closeAction) && !desktopGExited && alive(svcG)
      ? "passed"
      : "failed",
    {
      action: closeAction,
      window: closed.detail,
      desktop_alive: !desktopGExited,
      service_alive: alive(svcG),
    },
  );
  const second = spawn(desktopExe, [], { env: envG, cwd: work, stdio: "ignore" });
  const secondExit = await new Promise((resolve) => {
    const timer = setTimeout(() => resolve("still running"), 30_000);
    second.on("exit", (code) => {
      clearTimeout(timer);
      resolve(code);
    });
  });
  if (secondExit === "still running") killTree(second.pid);
  const focused = await until(
    "the first window to be shown by the second launch",
    () => lifeEvents().find((e) => e.event === "second_instance"),
    20_000,
  ).catch(() => null);
  record(
    "G2 a second launch exits and shows the existing window",
    secondExit === 0 && focused?.pid === desktopG.pid && focused?.detail?.visible === true
      ? "passed"
      : "failed",
    {
      second_exit: secondExit,
      shown_by_pid: focused?.pid ?? null,
      window: focused?.detail ?? null,
      // Linux: the single-instance plugin needs a D-Bus session bus (CI runs dbus-run-session).
      session_bus: platform === "linux" ? Boolean(process.env.DBUS_SESSION_BUS_ADDRESS) : undefined,
    },
  );
  const doneG = await until(
    "download G to finish",
    () => {
      const g = dm(homeG, dlG, ["get", jobG]).json?.data;
      return g && ["completed", "failed", "cancelled"].includes(g.phase) ? g : null;
    },
    180_000,
  );
  const fileG = fs.existsSync(path.join(dlG, doneG.display_name))
    ? fs.readFileSync(path.join(dlG, doneG.display_name))
    : Buffer.alloc(0);
  record(
    "G3 the transfer continued while the window was closed, SHA-256 verified",
    doneG.phase === "completed" && sha256(fileG) === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    { phase: doneG.phase, sha256: sha256(fileG) },
  );

  // H. "Exit and stop downloads" (B2.2, ADR 0012), through the tray item's path and the real
  // confirmation dialog, during a transfer.
  const addH = dm(homeG, dlG, ["add", `${base}/slow.bin?slow=150&step=h`]);
  const jobH = addH.json?.data?.job_id;
  await until(
    "transfer H to start",
    () => Number(dm(homeG, dlG, ["get", jobH]).json?.data?.downloaded_bytes ?? 0) > 0,
    30_000,
  );
  fs.writeFileSync(exitWhen, "");
  const exitEvent = await until(
    "exit and stop to finish",
    () => lifeEvents().find((e) => e.event === "exit_and_stop"),
    60_000,
  );
  await until("desktop G to quit", () => desktopGExited, 20_000).catch(() => null);
  record(
    "H1 confirmed exit paused the transfer, the service was verified stopped, and the desktop quit",
    exitEvent.detail?.report?.stopped === true &&
      exitEvent.detail.report.paused_jobs === 1 &&
      !alive(svcG) &&
      desktopGExited
      ? "passed"
      : "failed",
    {
      report: exitEvent.detail?.report ?? exitEvent.detail,
      service_alive: alive(svcG),
      desktop_exited: desktopGExited,
    },
  );
  const svcH = spawn(bins.service, ["--data-dir", homeG, "--download-dir", dlG], {
    env: env(homeG, dlG),
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(svcH.pid);
  await until("restarted service", () => dm(homeG, dlG, ["hello"]).code === 0, 30_000);
  const afterH = dm(homeG, dlG, ["get", jobH]).json?.data;
  await sleep(2000);
  const laterH = dm(homeG, dlG, ["get", jobH]).json?.data;
  record(
    "H2 after a restart the same job is paused and does not transfer",
    afterH?.desired_state === "paused" &&
      laterH?.desired_state === "paused" &&
      afterH.downloaded_bytes === laterH.downloaded_bytes &&
      BigInt(afterH.downloaded_bytes) > 0n &&
      afterH.error === null
      ? "passed"
      : "failed",
    {
      desired_state: afterH?.desired_state,
      phase: afterH?.phase,
      downloaded_bytes: afterH?.downloaded_bytes,
      two_seconds_later: laterH?.downloaded_bytes,
    },
  );
  const resumed = dm(homeG, dlG, ["resume", jobH]);
  const doneH = await until(
    "download H to finish after Resume",
    () => {
      const g = dm(homeG, dlG, ["get", jobH]).json?.data;
      return g && ["completed", "failed", "cancelled"].includes(g.phase) ? g : null;
    },
    240_000,
  );
  const fileH = fs.existsSync(path.join(dlG, doneH.display_name))
    ? fs.readFileSync(path.join(dlG, doneH.display_name))
    : Buffer.alloc(0);
  record(
    "H3 explicit Resume completed it, SHA-256 verified",
    resumed.code === 0 &&
      doneH.phase === "completed" &&
      sha256(fileH) === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    { phase: doneH.phase, sha256: sha256(fileH) },
  );
  fs.writeFileSync(path.join(outDir, "lifecycle-g.json"), JSON.stringify(lifeEvents(), null, 2));
  killTree(svcH.pid);
  await until("service H to exit", () => !alive(svcH.pid), 20_000);

  // I. Cancel download and Remove from list through the packaged UI (B2.3, ADR 0013). A smoke-only
  // driver selects the row, opens the File menu, chooses the item and confirms the dialog.
  const homeI = path.join(work, "i-data");
  const dlI = path.join(work, "i-downloads");
  fs.mkdirSync(dlI, { recursive: true });
  const commandsI = path.join(work, "commands-i.jsonl");
  const desktopI = spawn(desktopExe, [], {
    env: {
      ...env(homeI, dlI),
      DOWNOVA_SMOKE_FILE: path.join(work, "smoke-i.json"),
      DOWNOVA_SMOKE_COMMANDS_FILE: commandsI,
      DOWNOVA_SMOKE_LIFECYCLE_FILE: path.join(outDir, "lifecycle-i.jsonl"),
    },
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(desktopI.pid);
  await until("desktop I connected", () => fs.existsSync(path.join(work, "smoke-i.json")), 90_000);
  spawned.push(servicePid(homeI));
  const ui = (op, job) => fs.appendFileSync(commandsI, JSON.stringify({ op, job }) + "\n");
  const getI = (job) => dm(homeI, dlI, ["get", job]).json?.data;
  const listI = () => (dm(homeI, dlI, ["list"]).json?.data?.items ?? []).map((d) => d.job_id);

  const doneI = await download(homeI, dlI, `${base}/small.bin?step=i`, bodies["small.bin"]);
  const slowI = dm(homeI, dlI, ["add", `${base}/slow.bin?slow=150&step=i`]).json?.data?.job_id;
  await until("transfer I to start", () => Number(getI(slowI)?.downloaded_bytes ?? 0) > 0, 30_000);
  ui("cancel", slowI);
  const cancelledI = await until(
    "the UI cancel to be acknowledged",
    () => (getI(slowI)?.phase === "cancelled" ? getI(slowI) : null),
    30_000,
  );
  await sleep(1500);
  const afterCancel = getI(slowI);
  record(
    "I1 Cancel download (File menu + confirmation) stopped the slow transfer",
    cancelledI.desired_state === "cancelled" &&
      afterCancel.phase === "cancelled" &&
      afterCancel.downloaded_bytes === cancelledI.downloaded_bytes &&
      afterCancel.attempt_id === null
      ? "passed"
      : "failed",
    {
      downloaded_bytes_at_cancel: cancelledI.downloaded_bytes,
      one_and_a_half_seconds_later: afterCancel.downloaded_bytes,
    },
  );
  const finalI = path.join(dlI, getI(doneI.job).display_name);
  // One command at a time: each goes through its own menu and dialog.
  ui("remove", doneI.job);
  await until("the completed job's removal", () => !listI().includes(doneI.job), 30_000);
  ui("remove", slowI);
  await until("the cancelled job's removal", () => !listI().includes(slowI), 30_000);
  const keptI = fs.existsSync(finalI) ? fs.readFileSync(finalI) : Buffer.alloc(0);
  record(
    "I2 Remove from list removed the completed and the cancelled job and kept the file byte-exact",
    sha256(keptI) === doneI.expected && getI(doneI.job)?.removed === true && listI().length === 0
      ? "passed"
      : "failed",
    { sha256: sha256(keptI), removed: getI(doneI.job)?.removed ?? null, list: listI() },
  );
  killTree(desktopI.pid);
  killTree(servicePid(homeI));
  await until("desktop I to exit", () => !alive(desktopI.pid), 20_000);
  const svcI = spawn(bins.service, ["--data-dir", homeI, "--download-dir", dlI], {
    env: env(homeI, dlI),
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(svcI.pid);
  await until("restarted service I", () => dm(homeI, dlI, ["hello"]).code === 0, 30_000);
  const keptAfter = fs.existsSync(finalI) ? fs.readFileSync(finalI) : Buffer.alloc(0);
  record(
    "I3 after a restart both stay out of the list, the file is unchanged, and get still answers",
    listI().length === 0 &&
      sha256(keptAfter) === doneI.expected &&
      getI(doneI.job)?.removed === true &&
      getI(doneI.job)?.phase === "completed"
      ? "passed"
      : "failed",
    { list: listI(), sha256: sha256(keptAfter) },
  );
  killTree(svcI.pid);

  // R. browser registration through the production flow (B2.5)
  const homeR = path.join(work, "r-data");
  const dlR = path.join(work, "r-downloads");
  fs.mkdirSync(dlR, { recursive: true });
  const before = hostReg(["status", ...BROWSERS]);
  const ownerBefore = Object.fromEntries(before.rows.map((r) => [r.browser, r.result?.owner]));
  if (platform !== "win32")
    record(
      "R1a nothing registered before the first launch (package scripts write no per-user files)",
      Object.values(ownerBefore).every((o) => o === "absent") ? "passed" : "failed",
      { owners: ownerBefore },
    );
  else
    record(
      "R1a the NSIS installer's hook registered Chrome and Edge for the installing user",
      Object.values(ownerBefore).every((o) => o === "ours") && before.rows.length === 2
        ? "passed"
        : "failed",
      { owners: ownerBefore },
    );
  const smokeFileR = path.join(work, "smoke-r.json");
  const watchFile = path.join(work, "watch-r.json");
  const handoffUrl = `${base}/handoff.bin?slow=40`;
  const desktopR = spawn(desktopExe, [], {
    env: {
      ...env(homeR, dlR),
      ...regEnv,
      DOWNOVA_SKIP_BROWSER_SETUP: "",
      DOWNOVA_SMOKE_FILE: smokeFileR,
      DOWNOVA_SMOKE_WATCH_URL: handoffUrl,
      DOWNOVA_SMOKE_WATCH_FILE: watchFile,
    },
    cwd: work,
    stdio: "ignore",
  });
  spawned.push(desktopR.pid);
  const setupFile = path.join(homeR, "browser-setup.json");
  const setup = await until(
    "the desktop's first-launch browser registration",
    () => (fs.existsSync(setupFile) ? JSON.parse(fs.readFileSync(setupFile, "utf8")) : null),
    90_000,
  );
  fs.writeFileSync(path.join(outDir, "browser-setup.json"), JSON.stringify(setup, null, 2));
  record(
    "R1b the desktop's first launch registered Chrome and Edge for the current user",
    setup.complete === true && samePath(setup.host, bins.host) ? "passed" : "failed",
    { complete: setup.complete, exit_code: setup.exit_code, host: setup.host },
  );
  const after = hostReg(["status", ...BROWSERS]);
  const manifests = {};
  const checks = after.rows.map((row) => {
    const m = row.result?.manifest;
    let doc = null;
    try {
      doc = JSON.parse(fs.readFileSync(m, "utf8"));
      manifests[row.browser] = m;
    } catch {
      doc = null;
    }
    const mode = m && fs.existsSync(m) ? fs.statSync(m).mode & 0o777 : null;
    const hostMode = fs.statSync(bins.host).mode & 0o777;
    return {
      browser: row.browser,
      owner: row.result?.owner,
      registry_owner: row.result?.registry_owner?.owner ?? null,
      registry_points_to_manifest:
        platform === "win32" ? samePath(row.result?.registry_value ?? "", m ?? "") : null,
      name: doc?.name,
      type: doc?.type,
      path_is_installed_host: doc ? samePath(doc.path, bins.host) : false,
      origins: doc?.allowed_origins,
      manifest_mode: mode === null ? null : mode.toString(8),
      host_executable: platform === "win32" ? null : (hostMode & 0o111) !== 0,
      in_isolated_location: m ? m.startsWith(platform === "win32" ? regLocal : regHome) : false,
    };
  });
  record(
    "R1c manifests: owned by this installation, exact name/type/path/origins, permissions, location",
    checks.length === 2 &&
      checks.every(
        (c) =>
          c.owner === "ours" &&
          c.name === "com.downova.host" &&
          c.type === "stdio" &&
          c.path_is_installed_host &&
          JSON.stringify(c.origins) === JSON.stringify([DEV_ORIGIN]) &&
          c.in_isolated_location &&
          (platform === "win32"
            ? c.registry_owner === "ours" && c.registry_points_to_manifest
            : c.manifest_mode === "644" && c.host_executable),
      )
      ? "passed"
      : "failed",
    { checks },
  );
  const reportR = await until(
    "desktop smoke report (R)",
    () => (fs.existsSync(smokeFileR) ? JSON.parse(fs.readFileSync(smokeFileR, "utf8")) : null),
    90_000,
  );
  record(
    "R2 desktop window loaded and its service is ready",
    reportR.window?.title === "Downova" && reportR.service?.status === "connected"
      ? "passed"
      : "failed",
    { window: reportR.window, service: reportR.service?.status },
  );
  spawned.push(servicePid(homeR));

  // R3: the real browsers, using only the product's registration.
  const e2eRuns = [
    { kind: "chrome", url: handoffUrl, body: "handoff.bin" },
    { kind: "edge", url: `${base}/handoff-edge.bin`, body: "handoff-edge.bin" },
  ];
  for (const run of e2eRuns) {
    const out = path.join(outDir, `e2e-${run.kind}`);
    // Asynchronous: the fixture server lives in this process and must keep answering.
    const r = await runChild(
      process.execPath,
      [
        path.join(import.meta.dirname, "..", "e2e", "chrome-handoff.mjs"),
        "--kind",
        run.kind,
        "--installed-dir",
        binDir,
        "--data-dir",
        homeR,
        "--download-dir",
        dlR,
        "--product-registration",
        "--handoff-only",
        "--menu-url",
        run.url,
        "--menu-sha256",
        sha256(bodies[run.body]),
        "--out",
        out,
      ],
      {
        env: {
          ...env(homeR, dlR),
          ...regEnv,
          DOWNOVA_E2E_LOCALAPPDATA: process.env.LOCALAPPDATA ?? "",
        },
        cwd: work,
        stdio: "inherit",
      },
      300_000,
    );
    let e2e = null;
    try {
      e2e = JSON.parse(fs.readFileSync(path.join(out, "results.json"), "utf8"));
    } catch {
      e2e = null;
    }
    if (!e2e) {
      record(`R3 ${run.kind}: real-browser hand-off`, "failed", {
        exit: r.status,
        killed_after_300_s_without_results: r.timedOut,
      });
      continue;
    }
    if (e2e.verdict === "unavailable") {
      record(`R3 ${run.kind}: real-browser hand-off`, "unavailable", {
        reason: `${run.kind} is not installed on this machine`,
      });
      continue;
    }
    for (const st of e2e.steps) {
      const { step, status, ...detail } = st;
      record(`R3 ${run.kind} (${e2e.browser_version ?? "?"}): ${step}`, status, detail);
    }
  }

  // R4: the desktop observed the Chrome hand-off through downloads_list.
  const watch = await until(
    "the desktop to observe the hand-off",
    () => (fs.existsSync(watchFile) ? JSON.parse(fs.readFileSync(watchFile, "utf8")) : null),
    60_000,
  ).catch(() => null);
  const partial = (watch?.samples ?? []).filter(
    (x) =>
      x.phase === "transferring" &&
      BigInt(x.downloaded_bytes) > 0n &&
      x.total_bytes !== null &&
      BigInt(x.downloaded_bytes) < BigInt(x.total_bytes),
  );
  const fileR =
    watch?.job && fs.existsSync(path.join(dlR, watch.job.display_name))
      ? fs.readFileSync(path.join(dlR, watch.job.display_name))
      : Buffer.alloc(0);
  const handoffJobs = (dm(homeR, dlR, ["list"]).json?.data?.items ?? []).filter((j) =>
    j.source_display.includes("/handoff.bin"),
  );
  record(
    "R4 desktop downloads_list showed the browser hand-off's progress; one job; SHA-256",
    watch?.job?.phase === "completed" &&
      partial.length > 0 &&
      handoffJobs.length === 1 &&
      sha256(fileR) === sha256(bodies["handoff.bin"])
      ? "passed"
      : "failed",
    {
      phase: watch?.job?.phase ?? null,
      samples: watch?.samples?.length ?? 0,
      partial_samples: partial.length,
      jobs_for_link: handoffJobs.length,
      sha256: sha256(fileR),
    },
  );
  screenshot("desktop-after-browser-handoff", desktopR.pid);
  killTree(desktopR.pid);

  // R5: repair.
  const chromeManifest = manifests.chrome;
  const edgeManifest = manifests.edge;
  const good = chromeManifest ? fs.readFileSync(chromeManifest) : Buffer.alloc(0);
  const goodEdge = edgeManifest ? fs.readFileSync(edgeManifest) : Buffer.alloc(0);
  const repair = (extra = []) => hostReg(["repair", ...BROWSERS, ...extra]);
  const sameBytes = (f, b) => fs.existsSync(f) && fs.readFileSync(f).equals(b);
  fs.rmSync(chromeManifest, { force: true });
  const damaged = JSON.parse(goodEdge.toString("utf8"));
  damaged.allowed_origins = [];
  fs.writeFileSync(edgeManifest, JSON.stringify(damaged));
  if (platform === "win32") spawnSync("reg", ["delete", HOST_KEYS[1], "/f"]);
  const r1 = repair();
  record(
    "R5a repair restores a missing manifest, a damaged owned manifest (and a missing registry value)",
    r1.code === 0 &&
      sameBytes(chromeManifest, good) &&
      sameBytes(edgeManifest, goodEdge) &&
      (platform !== "win32" || samePath(regQuery(HOST_KEYS[1]) ?? "", edgeManifest))
      ? "passed"
      : "failed",
    {
      code: r1.code,
      rows: r1.rows.map((r) => ({
        browser: r.browser,
        ok: r.ok,
        previous: r.result?.previous?.owner,
      })),
    },
  );
  fs.writeFileSync(chromeManifest, "{ not json");
  const r2 = repair();
  const untouched = fs.readFileSync(chromeManifest, "utf8") === "{ not json";
  const r3 = repair(["--replace"]);
  record(
    "R5b an unreadable manifest is refused (exit 3, untouched) and replaced only with --replace",
    r2.code === 3 && untouched && r3.code === 0 && sameBytes(chromeManifest, good)
      ? "passed"
      : "failed",
    { refused_exit: r2.code, untouched, replace_exit: r3.code },
  );
  killTree(servicePid(homeR));

  // D. uninstall
  // Unrelated entries next to ours, which uninstalling must not touch.
  const unrelated = path.join(
    path.dirname(chromeManifest),
    "com.example.downova_smoke_unrelated.json",
  );
  fs.writeFileSync(
    unrelated,
    JSON.stringify({ name: "com.example.downova_smoke_unrelated", path: "/nowhere" }),
  );
  if (platform === "win32")
    spawnSync("reg", ["add", UNRELATED_KEY, "/ve", "/t", "REG_SZ", "/d", unrelated, "/f"]);
  if (platform !== "win32") {
    // No per-user uninstall hook exists for these packages (dpkg runs as root; macOS apps are
    // dragged to the Trash): the documented step is the host's own unregister.
    const un = hostReg(["unregister", ...BROWSERS]);
    record(
      "D0 documented `dm-browser-host unregister` before removing the package (no uninstall hook)",
      un.code === 0 ? "passed" : "failed",
      {
        code: un.code,
        rows: un.rows.map((r) => ({
          browser: r.browser,
          removed: r.result?.removed,
          kept: r.result?.kept,
        })),
      },
    );
  }
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
  const gone = [chromeManifest, edgeManifest].every((m) => !fs.existsSync(m));
  const keysGone = HOST_KEYS.every((k) => regQuery(k) === null);
  const unrelatedKept =
    fs.existsSync(unrelated) && (platform !== "win32" || regQuery(UNRELATED_KEY) === unrelated);
  const dataKept =
    fs.existsSync(homeR) &&
    sha256(readOr(path.join(dlR, watch?.job?.display_name ?? "handoff.bin"))) ===
      sha256(bodies["handoff.bin"]);
  record(
    "D2 this installation's registration removed; unrelated entries, downloads and data kept",
    gone && keysGone && unrelatedKept && dataKept ? "passed" : "failed",
    {
      manifests_removed: gone,
      registry_values_removed: platform === "win32" ? keysGone : null,
      unrelated_kept: unrelatedKept,
      downloads_and_data_kept: dataKept,
      via: platform === "win32" ? "NSIS uninstaller hook" : "documented unregister",
    },
  );
} catch (e) {
  record("smoke run", "failed", { error: String(e?.message ?? e) });
  screenshot("failure");
} finally {
  for (const pid of spawned) killTree(pid);
  if (platform === "win32") restoreRegistry();
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
  publishRows(`Packaged smoke ${label} ${platform} ${process.arch}`, results);
  fs.writeFileSync(path.join(outDir, "summary.md"), md + "\n");
  console.log(`\n${md}\n`);
  process.exit(exitCode);
}
