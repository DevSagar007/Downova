#!/usr/bin/env node
// B3.4 real-Firefox hand-off test (BRW-08, ADR 0029).
//
//   node scripts/e2e/firefox-handoff.mjs [--firefox PATH] [--headed] [--out DIR]
//
// Drives a real Firefox with an isolated temporary profile over WebDriver BiDi (Firefox's own
// remote protocol; no Chromium, no mocked browser API). The e2e build of the Firefox extension
// (`vite build --mode firefox-e2e`) is installed as a *temporary* add-on (`webExtension.install`),
// which is how an unsigned development build loads; it is not signed distribution. The test reaches
// the extension's background (an event page in Firefox) from the extension's own popup page with
// `browser.runtime.getBackgroundPage()`. Firefox cannot click its native context menu from
// automation, so the test calls the background's registered menu listener with a link-click
// payload; everything after that is the real path: Firefox native messaging → the registered
// dm-browser-host (Firefox launch arguments) → dm-service → a controlled fixture.
//
// Rows:
//   F1  the temporary add-on installs with the development add-on ID the host allows
//   F2  its menu has "Download with Downova" and no sign-in or reconnect items; no capture or
//       cookie permission; private windows not allowed
//   F3  the popup's readiness check: popup → background → host → service
//   F4  menu hand-off → accepted (not "started") → one job → SHA-256
//   F5  lost acknowledgement → uncertain; the add-on's background is restarted (runtime.reload);
//       the stored hand-off is retried with the same key → accepted; one job for the link
//   F6  a download Firefox starts itself (a public link, a blob:) stays in Firefox: no record,
//       no job, the file completes in Firefox's download folder
//   F7  Chromium-only requests (capture settings, reconnect) are refused by the Firefox build
//   F8  host not registered → unavailable (host_missing), nothing sent
//   F9  service stopped → unavailable; nothing accepted
//
// Registration is the product's own command (`dm-browser-host register --browser firefox`):
// on Windows the per-user key HKCU\Software\Mozilla\NativeMessagingHosts\com.downova.host (any
// previous value is restored afterwards) with the manifest under a temporary root; on macOS/Linux
// the browser runs with a temporary HOME, so the manifest goes there. The service, data and
// download directories are temporary.
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const argv = process.argv.slice(2);
const arg = (n) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : undefined;
};
const platform = process.platform;
const exe = platform === "win32" ? ".exe" : "";
const ADDON_ID = "downova-dev@extension.downova.invalid";
const ADDON_UUID = "3c1f6a52-6d0e-4d1b-9a8e-5b2f4c7d9e10";
const BASE = `moz-extension://${ADDON_UUID}`;
const HOST_KEY = String.raw`HKCU\Software\Mozilla\NativeMessagingHosts\com.downova.host`;
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-firefox-e2e-"));
const outDir = path.resolve(arg("--out") ?? path.join(work, "results"));
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const results = { started_at: new Date().toISOString(), platform, steps: [] };
function record(step, status, detail = {}) {
  results.steps.push({ step, status, ...detail });
  console.log(`${status.toUpperCase().padEnd(11)} ${step} ${JSON.stringify(detail).slice(0, 500)}`);
}
async function until(what, fn, ms) {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < ms) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}: ${String(last?.message ?? last)}`);
}

// ------------------------------------------------------------------ Firefox
function findFirefox() {
  const given = arg("--firefox");
  const candidates = given
    ? [given]
    : platform === "win32"
      ? [
          String.raw`C:\Program Files\Mozilla Firefox\firefox.exe`,
          String.raw`C:\Program Files (x86)\Mozilla Firefox\firefox.exe`,
        ]
      : platform === "darwin"
        ? ["/Applications/Firefox.app/Contents/MacOS/firefox"]
        : ["/usr/lib/firefox/firefox", "/usr/bin/firefox", "/opt/firefox/firefox"];
  return candidates.find((p) => fs.existsSync(p));
}
function firefoxVersion(bin) {
  const ini = [
    path.join(path.dirname(bin), "application.ini"),
    path.join(path.dirname(bin), "..", "Resources", "application.ini"),
  ].find((p) => fs.existsSync(p));
  return ini ? /^Version=(.+)$/m.exec(fs.readFileSync(ini, "utf8"))?.[1]?.trim() : null;
}

// ------------------------------------------------------------------ fixture
const bodies = {
  "menu.bin": payload(3 * 1024 * 1024, 41),
  "lost.bin": payload(1024 * 1024, 42),
  "nohost.bin": payload(64 * 1024, 43),
  "down.bin": payload(64 * 1024, 44),
  "browser.bin": payload(256 * 1024, 45),
};
const html = (res, body) => {
  res.writeHead(200, { "Content-Type": "text/html" });
  res.end(`<!doctype html><meta charset="utf-8"><body>${body}</body>`);
  return true;
};
async function extra(req, res, url) {
  if (url.pathname === "/attach/browser.bin") {
    const b = bodies["browser.bin"];
    res.writeHead(200, {
      "Content-Type": "application/octet-stream",
      "Content-Disposition": 'attachment; filename="browser.bin"',
      "Content-Length": b.length,
    });
    res.end(b);
    return true;
  }
  if (url.pathname === "/blob-page")
    return html(
      res,
      '<script>const a=document.createElement("a");a.href=URL.createObjectURL(new Blob(["blob-bytes"]));a.download="made.txt";document.body.append(a);a.click()</script>',
    );
  if (url.pathname === "/link-page")
    return html(
      res,
      '<a id="go" href="/attach/browser.bin">download</a><script>go.click()</script>',
    );
  return false;
}
const { server, base } = await startFixtureServer(bodies, { extra });

// ------------------------------------------------------------------ service, CLI, host
const home = path.join(work, "data");
const dl = path.join(work, "downloads");
const browserDl = path.join(work, "firefox-downloads");
for (const d of [home, dl, browserDl]) fs.mkdirSync(d, { recursive: true });
const fakeHome = path.join(work, "home");
const env = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  DM_ARIA2_PATH: path.join(work, "no-such-aria2c"),
  // Debug service: the test vault, never the developer's credential store.
  DOWNOVA_VAULT: "file",
  ...(platform === "win32" ? {} : { HOME: fakeHome }),
};
const bin = (name) => path.join(root, "target", "debug", `${name}${exe}`);
function dm(args) {
  const r = spawnSync(bin("dm"), ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null };
  }
}
const jobs = () => dm(["list"]).json?.data?.items ?? [];
async function finished(jobId) {
  const done = await until(
    `job ${jobId}`,
    () => {
      const g = dm(["get", jobId]).json?.data;
      return ["completed", "failed", "cancelled"].includes(g?.phase) ? g : null;
    },
    120_000,
  );
  const file = path.join(dl, done.display_name);
  return {
    phase: done.phase,
    sha256: fs.existsSync(file) ? sha256(fs.readFileSync(file)) : null,
  };
}
let service = null;
function startService() {
  service = spawn(bin("dm-service"), ["--data-dir", home, "--download-dir", dl], {
    env,
    stdio: "ignore",
  });
  return until("service hello", () => dm(["hello"]).code === 0, 30_000);
}
function stopService() {
  dm(["shutdown"]);
  return until("service stop", () => service.exitCode !== null, 20_000).catch(() => service.kill());
}

const hostExe = path.join(work, "host", `dm-browser-host${exe}`);
const regRoot = path.join(work, "reg-root");
let previousKey;
function regValue() {
  const q = spawnSync("reg", ["query", HOST_KEY, "/ve"], { encoding: "utf8" });
  if (q.status !== 0) return null;
  return /REG_SZ\s+(.+)$/m.exec(q.stdout)?.[1]?.trim() ?? null;
}
function register() {
  const r = spawnSync(
    hostExe,
    [
      "register",
      "--browser",
      "firefox",
      "--host-path",
      hostExe,
      "--json",
      ...(platform === "win32" ? ["--root", regRoot] : ["--root", fakeHome]),
    ],
    { encoding: "utf8", env },
  );
  let row = {};
  try {
    row = JSON.parse(r.stdout.trim().split("\n").pop());
  } catch {
    // reported below
  }
  return { code: r.status, row };
}
function unregister() {
  return spawnSync(
    hostExe,
    [
      "unregister",
      "--browser",
      "firefox",
      "--host-path",
      hostExe,
      ...(platform === "win32" ? ["--root", regRoot] : ["--root", fakeHome]),
    ],
    { encoding: "utf8", env },
  ).status;
}
function restoreRegistry() {
  if (platform !== "win32" || previousKey === undefined) return;
  if (previousKey === null) spawnSync("reg", ["delete", HOST_KEY, "/f"]);
  else spawnSync("reg", ["add", HOST_KEY, "/ve", "/t", "REG_SZ", "/d", previousKey, "/f"]);
}

// ------------------------------------------------------------------ WebDriver BiDi
class Bidi {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    ws.addEventListener("message", (e) => {
      const m = JSON.parse(String(e.data));
      if (m.id && this.pending.has(m.id)) {
        const { resolve, reject } = this.pending.get(m.id);
        this.pending.delete(m.id);
        if (m.type === "error") reject(new Error(`${m.error}: ${m.message}`));
        else resolve(m.result);
      }
    });
  }
  send(method, params = {}, timeoutMs = 60_000) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => (clearTimeout(timer), resolve(v)),
        reject: (e) => (clearTimeout(timer), reject(e)),
      });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}
async function connect(port) {
  return until(
    "the BiDi endpoint",
    () =>
      new Promise((resolve) => {
        const ws = new WebSocket(`ws://127.0.0.1:${port}/session`);
        ws.addEventListener("open", () => resolve(ws));
        ws.addEventListener("error", () => resolve(null));
      }),
    60_000,
  );
}

let firefox = null;
let bidi = null;
let page = null; // the extension popup page's browsing context
/** Evaluates `body` (an async function body) in the extension page, with `bg` = its background. */
async function inExtension(body) {
  const r = await bidi.send("script.evaluate", {
    expression: `(async () => { const bg = await browser.runtime.getBackgroundPage(); const e2e = bg.downovaE2E; ${body} })().then((v) => JSON.stringify(v ?? null))`,
    target: { context: page },
    awaitPromise: true,
  });
  if (r.type === "exception")
    throw new Error(r.exceptionDetails?.text ?? "exception in the extension page");
  return JSON.parse(r.result.value);
}
/**
 * The extension's own page. Automation may not navigate a tab to an extension URL, so the e2e
 * build opens it whenever its background starts; this picks the newest such tab (`after` skips
 * contexts seen before a restart).
 */
async function openExtensionPage(after = new Set()) {
  page = await until(
    "the add-on's own page",
    async () => {
      const tree = await bidi.send("browsingContext.getTree", {});
      const ours = tree.contexts.filter((c) => c.url.startsWith(BASE) && !after.has(c.context));
      return ours.at(-1)?.context ?? null;
    },
    30_000,
  );
  return page;
}
const seenPages = async () =>
  new Set(
    (await bidi.send("browsingContext.getTree", {})).contexts
      .filter((c) => c.url.startsWith(BASE))
      .map((c) => c.context),
  );
async function settled(id) {
  return until(
    "the hand-off to settle",
    () =>
      inExtension(
        `const list = await e2e.list(); const r = ${id ? `list.find((x) => x.id === ${JSON.stringify(id)})` : "list[0]"}; return r && r.state !== "sending" ? r : null;`,
      ),
    60_000,
  );
}

let exitCode = 1;
try {
  const ffBin = findFirefox();
  if (!ffBin) {
    record("Firefox available", "unavailable", { reason: "no Firefox found; pass --firefox PATH" });
    throw new Error("no Firefox");
  }
  results.firefox = { path_kind: "native installation", version: firefoxVersion(ffBin) };

  // Builds: debug service, CLI and host; the Firefox extension in e2e mode.
  const build = spawnSync(
    "cargo",
    ["build", "--locked", "-p", "dm-service", "-p", "dm-cli", "-p", "dm-browser-host"],
    { cwd: root, stdio: "inherit" },
  );
  if (build.status !== 0) throw new Error("cargo build failed");
  const extDir = path.join(work, "extension");
  const vite = path.join(
    root,
    "apps/extension/node_modules/.bin",
    platform === "win32" ? "vite.cmd" : "vite",
  );
  const vb = spawnSync(
    vite,
    ["build", "--mode", "firefox-e2e", "--outDir", extDir, "--emptyOutDir"],
    { cwd: path.join(root, "apps/extension"), stdio: "inherit", shell: platform === "win32" },
  );
  if (vb.status !== 0) throw new Error("extension build failed");
  const manifest = JSON.parse(fs.readFileSync(path.join(extDir, "manifest.json"), "utf8"));
  const files = fs
    .readdirSync(extDir, { recursive: true })
    .map(String)
    .filter((f) => fs.statSync(path.join(extDir, f)).isFile())
    .sort();
  results.extension = {
    build: "vite --mode firefox-e2e (test hooks compiled in; temporary add-on)",
    id: manifest.browser_specific_settings?.gecko?.id,
    version: manifest.version,
    files_sha256: crypto
      .createHash("sha256")
      .update(files.map((f) => `${sha256(fs.readFileSync(path.join(extDir, f)))}  ${f}`).join("\n"))
      .digest("hex"),
  };
  const git = (...a) => spawnSync("git", a, { cwd: root, encoding: "utf8" }).stdout.trim();
  results.source = {
    commit: git("rev-parse", "HEAD"),
    uncommitted_paths: git("status", "--porcelain").split("\n").filter(Boolean).length,
    host_and_service: "debug builds from this tree",
  };

  fs.mkdirSync(path.dirname(hostExe), { recursive: true });
  fs.copyFileSync(bin("dm-browser-host"), hostExe);
  await startService();
  record("service started (isolated data and download directories)", "passed");

  if (platform === "win32") previousKey = regValue();
  const reg = register();
  record(
    "host registered for Firefox by the product command (allowed_extensions)",
    reg.code === 0 && reg.row.ok === true ? "passed" : "failed",
    { code: reg.code, manifest_kind: reg.row.result ? "firefox" : null },
  );
  const written = reg.row.result?.manifest
    ? JSON.parse(fs.readFileSync(reg.row.result.manifest, "utf8"))
    : {};

  // Profile: fixed extension UUID, downloads into a temporary folder, no first-run noise.
  const profile = path.join(work, "profile");
  fs.mkdirSync(profile, { recursive: true });
  const prefs = {
    "extensions.webextensions.uuids": JSON.stringify({ [ADDON_ID]: ADDON_UUID }),
    "browser.download.dir": browserDl,
    "browser.download.folderList": 2,
    "browser.download.useDownloadDir": true,
    "browser.download.always_ask_before_handling_new_types": false,
    "browser.shell.checkDefaultBrowser": false,
    "browser.startup.homepage_override.mstone": "ignore",
    "browser.aboutwelcome.enabled": false,
    "datareporting.policy.dataSubmissionEnabled": false,
    "toolkit.telemetry.reportingpolicy.firstRun": false,
    "app.update.disabledForTesting": true,
  };
  fs.writeFileSync(
    path.join(profile, "user.js"),
    Object.entries(prefs)
      .map(([k, v]) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`)
      .join("\n"),
  );
  const port = 9300 + Math.floor(Math.random() * 500);
  firefox = spawn(
    ffBin,
    [
      "-profile",
      profile,
      "-no-remote",
      "-new-instance",
      `--remote-debugging-port=${port}`,
      ...(argv.includes("--headed") ? [] : ["-headless"]),
    ],
    // Test-only: Firefox lets remote automation evaluate script in extension pages only with
    // this switch. It applies to this test process and its throwaway profile alone.
    { env: { ...env, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: "1" }, stdio: ["ignore", "ignore", "pipe"] },
  );
  results.test_only = [
    "temporary add-on (webExtension.install), not a signed install",
    "MOZ_REMOTE_ALLOW_SYSTEM_ACCESS=1 for the test Firefox, to evaluate in the add-on page",
    "the context menu is invoked through the background listener (automation cannot click native menus)",
    "the e2e build opens its own page when its background starts",
  ];
  let ffErr = "";
  firefox.stderr.on("data", (d) => (ffErr = (ffErr + d).slice(-4000)));
  const ws = await connect(port);
  bidi = new Bidi(ws);
  await bidi.send("session.new", { capabilities: {} });
  const installed = await bidi.send("webExtension.install", {
    extensionData: { type: "path", path: extDir },
  });
  record(
    "F1 the temporary add-on installs with the development add-on ID the host allows",
    installed.extension === ADDON_ID && written.allowed_extensions?.includes(ADDON_ID)
      ? "passed"
      : "failed",
    { id: installed.extension, host_allowed_extensions: written.allowed_extensions },
  );
  await openExtensionPage();

  // F2: what the Firefox build offers.
  const offered = await inExtension(`
    const perms = await browser.permissions.getAll();
    return {
      target: e2e.target, features: e2e.features,
      menu: await e2e.menuExists("downova-download-link"),
      signInMenu: await e2e.menuExists("downova-download-signed-in"),
      reconnectMenu: await e2e.menuExists("downova-reconnect-signed-in"),
      permissions: perms.permissions, origins: perms.origins,
      privateAllowed: await browser.extension.isAllowedIncognitoAccess(),
    };`);
  record(
    "F2 Firefox build: the hand-off menu only (no sign-in or reconnect items), no capture or cookie permission, private windows not allowed",
    offered.target === "firefox" &&
      offered.menu === true &&
      offered.signInMenu === false &&
      offered.reconnectMenu === false &&
      !offered.permissions.some((p) => ["cookies", "downloads", "webRequest"].includes(p)) &&
      offered.origins.length === 0 &&
      offered.privateAllowed === false
      ? "passed"
      : "failed",
    offered,
  );

  // F3: readiness through the popup.
  const ready = await until(
    "the popup's service line",
    async () => {
      const r = await bidi.send("script.evaluate", {
        expression: `document.getElementById("service")?.textContent ?? ""`,
        target: { context: page },
        awaitPromise: false,
      });
      const t = r.result?.value ?? "";
      return t && !t.startsWith("Checking") ? t : null;
    },
    30_000,
  );
  record(
    "F3 popup readiness check: popup → background → host (Firefox launch arguments) → service",
    ready.startsWith("Service ready.") ? "passed" : "failed",
    { text: ready },
  );

  // F4: the menu hand-off.
  const menuUrl = `${base}/menu.bin?token=SYNTHETIC-SECRET-ff`;
  await inExtension(
    `e2e.menuClick({ menuItemId: e2e.menuId, linkUrl: ${JSON.stringify(menuUrl)} }); return true;`,
  );
  const menu = await settled();
  const menuDone = menu.jobId ? await finished(menu.jobId) : null;
  const menuJobs = jobs().filter((j) => j.source_display?.includes("/menu.bin"));
  record(
    'F4 menu hand-off → real native messaging → host → service: accepted (not "started"), one job, SHA-256',
    menu.state === "accepted" &&
      menu.code === "accepted" &&
      menuDone?.phase === "completed" &&
      menuDone.sha256 === sha256(bodies["menu.bin"]) &&
      menuJobs.length === 1
      ? "passed"
      : "failed",
    {
      state: menu.state,
      job: menu.jobId,
      phase: menuDone?.phase,
      sha256: menuDone?.sha256,
      jobs_for_link: menuJobs.length,
    },
  );

  // F5: lost acknowledgement, background restart, same-key retry.
  const lostUrl = `${base}/lost.bin`;
  const lost = await inExtension(`return e2e.start(${JSON.stringify(lostUrl)}, 1);`);
  const before = await seenPages();
  await inExtension(`browser.runtime.reload(); return true;`).catch(() => null);
  await openExtensionPage(before);
  await until("the restarted background", () => inExtension(`return !!e2e;`), 30_000);
  const afterRestart = await inExtension(
    `return (await e2e.list()).find((r) => r.id === ${JSON.stringify(lost.id)}) ?? null;`,
  );
  await inExtension(`await e2e.retry(${JSON.stringify(lost.id)}); return true;`);
  const retried = await settled(lost.id);
  const lostDone = retried.jobId ? await finished(retried.jobId) : null;
  const lostJobs = jobs().filter((j) => j.source_display?.includes("/lost.bin"));
  record(
    "F5 lost acknowledgement → uncertain; the add-on background restarts; the stored hand-off is retried with the same key → one job, SHA-256",
    lost.state === "uncertain" &&
      afterRestart?.state === "uncertain" &&
      afterRestart.idempotencyKey === lost.idempotencyKey &&
      retried.state === "accepted" &&
      lostJobs.length === 1 &&
      lostDone?.sha256 === sha256(bodies["lost.bin"])
      ? "passed"
      : "failed",
    {
      first: lost.state,
      after_restart: afterRestart?.state,
      same_key: afterRestart?.idempotencyKey === lost.idempotencyKey,
      retry: retried.state,
      jobs_for_link: lostJobs.length,
    },
  );

  // F6: downloads Firefox starts itself stay in Firefox.
  const handoffsBefore = (await inExtension(`return e2e.list();`)).length;
  const jobsBefore = jobs().length;
  const tab = (await bidi.send("browsingContext.create", { type: "tab" })).context;
  await bidi.send("browsingContext.navigate", {
    context: tab,
    url: `${base}/link-page`,
    wait: "complete",
  });
  const browserFile = path.join(browserDl, "browser.bin");
  await until(
    "Firefox to save its own download",
    () =>
      fs.existsSync(browserFile) && fs.statSync(browserFile).size === bodies["browser.bin"].length,
    60_000,
  ).catch(() => null);
  await bidi.send("browsingContext.navigate", {
    context: tab,
    url: `${base}/blob-page`,
    wait: "complete",
  });
  const blobFile = path.join(browserDl, "made.txt");
  await until("Firefox to save the blob", () => fs.existsSync(blobFile), 30_000).catch(() => null);
  await sleep(1000);
  const handoffsAfter = (await inExtension(`return e2e.list();`)).length;
  record(
    "F6 a public download and a blob: download started by Firefox stay in Firefox (no hand-off, no job; Firefox saved both)",
    fs.existsSync(browserFile) &&
      sha256(fs.readFileSync(browserFile)) === sha256(bodies["browser.bin"]) &&
      fs.existsSync(blobFile) &&
      handoffsAfter === handoffsBefore &&
      jobs().length === jobsBefore
      ? "passed"
      : "failed",
    {
      browser_file: fs.existsSync(browserFile),
      blob_file: fs.existsSync(blobFile),
      new_handoffs: handoffsAfter - handoffsBefore,
      new_jobs: jobs().length - jobsBefore,
    },
  );
  await bidi.send("browsingContext.close", { context: tab }).catch(() => null);

  // F7: Chromium-only requests are refused by the Firefox build.
  const refused = await inExtension(`
    const a = await browser.runtime.sendMessage({ type: "reconnectGet", id: "r_x" });
    const b = await browser.runtime.sendMessage({ type: "captureSave", settings: { enabled: true, sites: ["127.0.0.1"] } });
    return { reconnect: a, capture: b };`);
  record(
    "F7 Chromium-only requests (reconnect, capture settings) are refused by the Firefox build",
    refused.reconnect?.unsupported === true && refused.capture?.unsupported === true
      ? "passed"
      : "failed",
    refused,
  );

  // F8: host not registered.
  const unreg = unregister();
  if (platform === "win32") spawnSync("reg", ["delete", HOST_KEY, "/f"]);
  // The browser's own wording for a missing host (recorded: it differs between browsers).
  const missingMessage = await inExtension(
    `return browser.runtime.sendNativeMessage("com.downova.host", { v: 1, request_id: "req_probe0001", op: "hello" }).then(() => "answered", (e) => String(e?.message ?? e));`,
  );
  const noHost = await inExtension(`return e2e.start(${JSON.stringify(`${base}/nohost.bin`)});`);
  const noHostJobs = jobs().filter((j) => j.source_display?.includes("/nohost.bin"));
  record(
    "F8 host not registered → unavailable (host_missing); nothing reached a service",
    unreg === 0 &&
      noHost.state === "unavailable" &&
      noHost.code === "host_missing" &&
      noHostJobs.length === 0
      ? "passed"
      : "failed",
    {
      unregister_exit: unreg,
      browser_message: missingMessage,
      state: noHost.state,
      code: noHost.code,
      jobs: noHostJobs.length,
    },
  );
  const again = register();
  if (again.code !== 0) throw new Error("re-registration failed");

  // F9: service stopped.
  await stopService();
  const down = await inExtension(`return e2e.start(${JSON.stringify(`${base}/down.bin`)});`);
  await startService();
  const downJobs = jobs().filter((j) => j.source_display?.includes("/down.bin"));
  record(
    "F9 service stopped → unavailable; nothing accepted",
    down.state === "unavailable" && downJobs.length === 0 ? "passed" : "failed",
    { state: down.state, code: down.code, jobs: downJobs.length },
  );
  results.firefox_stderr_tail = ffErr.slice(-1200);
  exitCode = results.steps.every((s) => s.status === "passed") ? 0 : 1;
} catch (e) {
  record("run", "failed", { error: String(e?.message ?? e) });
} finally {
  try {
    await bidi?.send("browser.close", {}, 10_000);
  } catch {
    // closed below
  }
  if (firefox && firefox.exitCode === null) firefox.kill();
  try {
    if (fs.existsSync(hostExe)) unregister();
  } catch {
    // best effort
  }
  restoreRegistry();
  results.registry_restored =
    platform === "win32" ? { previous: previousKey ?? null, now: regValue() } : "n/a";
  if (service && service.exitCode === null) await stopService();
  server.close();
  results.finished_at = new Date().toISOString();
  results.verdict = exitCode === 0 ? "passed" : "failed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(`verdict ${results.verdict} (${outDir})`);
  process.exit(exitCode);
}
