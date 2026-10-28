#!/usr/bin/env node
// B0.6 real-browser hand-off test (docs/reports/b0.6-browser-handoff.md).
//
//   node scripts/e2e/chrome-handoff.mjs [--kind chrome|edge] [--browser PATH] [--headed] [--out DIR]
//
// Installed mode (B2.5, used by scripts/smoke/packaged-smoke.mjs):
//   --installed-dir DIR --data-dir HOME --download-dir DL --product-registration --handoff-only
//   [--menu-url URL --menu-sha256 HEX]
// uses the packaged host and CLI in DIR and the service already running for HOME (started by the
// packaged desktop), never registers anything itself, and runs steps 1–3 only. The browser finds
// the host through the registration the product created:
//   - Windows: the per-user HKCU key (the profile is temporary);
//   - macOS/Linux: the manifest in the browser's default user-data directory under $HOME (the
//     caller isolates HOME); the browser is started with exactly that directory. On Linux that
//     profile is Chrome's default, where Chrome 136+ refuses DevTools, so the browser process gets
//     an isolated config home while --user-data-dir stays on the product directory.
//
// Drives a real Chrome (or Chromium/Edge) with an isolated temporary profile over the DevTools
// pipe. Everything after the context-menu click is the real path: the extension's service worker
// → Chrome native messaging → the registered dm-browser-host → dm-service → a controlled fixture.
//   0. The DevTools pipe answers (Browser.getVersion) with a bounded wait.
//   1. The unpacked extension (e2e build) loads and its ID equals the host's allowlisted dev ID.
//   2. The popup's readiness check reports "Service ready" (popup → worker → host → service).
//   3. Context-menu hand-off: Chrome cannot click its native menu from automation, so the test
//      calls the worker's registered onClicked listener with a link-click payload. The job is
//      accepted, the file completes and its SHA-256 matches; the popup shows the acceptance.
//   4. Lost acknowledgement: a hand-off with a 1 ms acknowledgement timeout is "uncertain"; Chrome
//      is closed and restarted (new worker, same profile); the stored hand-off is retried with the
//      same key and the service holds exactly one job for it.
//   5. Browser closed after acceptance: a slow transfer keeps running and completes (SHA-256).
//
// Registration is reversible:
//   - Windows: the per-user HKCU key, which the browser reads; any previous value is restored
//     afterwards.
//   - macOS/Linux: the manifest goes into the temporary profile's own NativeMessagingHosts folder.
// The service, data and download directories are temporary.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { browserRuntimeEnv } from "../lib/browser-profile.mjs";
import { payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const argv = process.argv.slice(2);
const arg = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const platform = process.platform;
const exe = platform === "win32" ? ".exe" : "";
const DEV_ID = "bkmpnfclpdmcbdgmeakmnoilmkdmdppf";
const HOST_NAME = "com.downova.host";
const headed = argv.includes("--headed");
const kind = arg("--kind") ?? "chrome";
const installedDir = arg("--installed-dir");
const productRegistration = argv.includes("--product-registration");
const handoffOnly = argv.includes("--handoff-only");
if (productRegistration && !installedDir)
  throw new Error("--product-registration needs --installed-dir");
if (!["chrome", "edge"].includes(kind)) throw new Error(`--kind ${kind}: chrome or edge`);

function findBrowser() {
  const given = arg("--browser") ?? process.env.DM_CHROME_PATH;
  if (given) return given;
  const candidates =
    {
      chrome: {
        win32: [
          // A caller that isolates LOCALAPPDATA (the packaged smoke) passes the real one for per-user
          // browser installs.
          path.join(
            process.env.DOWNOVA_E2E_LOCALAPPDATA ?? process.env.LOCALAPPDATA ?? "",
            "Google/Chrome/Application/chrome.exe",
          ),
          "C:/Program Files/Google/Chrome/Application/chrome.exe",
          "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
        ],
        darwin: ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
        linux: ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium"],
      },
      edge: {
        win32: [
          "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
          "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
        ],
        darwin: ["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
        linux: ["/usr/bin/microsoft-edge", "/usr/bin/microsoft-edge-stable"],
      },
    }[kind][platform] ?? [];
  return candidates.find((p) => fs.existsSync(p));
}
const browserPath = findBrowser();
const browserVersion = (() => {
  if (!browserPath) return null;
  if (platform === "win32")
    return execFileSync(
      "powershell",
      ["-NoProfile", "-Command", `(Get-Item '${browserPath}').VersionInfo.ProductVersion`],
      { encoding: "utf8" },
    ).trim();
  return execFileSync(browserPath, ["--version"], { encoding: "utf8" }).trim();
})();
const label = `${kind}-${platform}-${process.arch}`;
const outDir = path.resolve(arg("--out") ?? path.join(root, "smoke-results", `e2e-${label}`));
fs.mkdirSync(outDir, { recursive: true });
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-chrome-e2e-"));

const results = {
  label,
  platform,
  arch: process.arch,
  browser: browserPath,
  browser_version: browserVersion,
  headless: !headed,
  started_at: new Date().toISOString(),
  steps: [],
};
const logLines = [];
const e2eLogFile = path.join(outDir, "e2e.log");
const log = (msg) => {
  const line = `[${new Date().toISOString()}] ${msg}`;
  console.log(line);
  logLines.push(line);
  // Written as the run proceeds so an outer timeout that kills this process still leaves a tail.
  fs.appendFileSync(e2eLogFile, line + "\n");
};
function record(step, status, detail = {}) {
  results.steps.push({ step, status, ...detail });
  log(`${status.toUpperCase().padEnd(11)} ${step} ${JSON.stringify(detail)}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(what, fn, ms) {
  const deadline = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await sleep(250);
  }
}

// ------------------------------------------------------------------ controlled fixture server
const bodies = {
  "menu.bin": payload(3 * 1024 * 1024, 7),
  "lostack.bin": payload(1024 * 1024, 5),
  "slow.bin": payload(6 * 1024 * 1024, 11),
};
const { server, base } = await startFixtureServer(bodies);

// ------------------------------------------------------------------ service and CLI
const home = arg("--data-dir") ?? path.join(work, "data");
const dl = arg("--download-dir") ?? path.join(work, "downloads");
fs.mkdirSync(dl, { recursive: true });
const env = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  DM_ARIA2_PATH: path.join(work, "no-such-aria2c"),
};
let browserEnv = env;
const bin = (name) =>
  path.join(installedDir ?? path.join(root, "target", "debug"), `${name}${exe}`);
function dm(args) {
  const r = spawnSync(bin("dm"), ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
  });
  let json = null;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    // reported by the caller
  }
  return { code: r.status, json, stderr: r.stderr };
}
const jobs = () => dm(["list"]).json?.data?.items ?? [];
async function finished(jobId, ms = 120_000) {
  const done = await until(
    `job ${jobId} to finish`,
    () => {
      const g = dm(["get", jobId]).json?.data;
      return ["completed", "failed", "cancelled"].includes(g?.phase) ? g : null;
    },
    ms,
  );
  const file = path.join(dl, done.display_name);
  return {
    phase: done.phase,
    sha256: fs.existsSync(file) ? sha256(fs.readFileSync(file)) : null,
  };
}
function killTree(pid) {
  if (!pid) return;
  if (platform === "win32") spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"]);
  else spawnSync("kill", ["-9", String(pid)]);
}
function alive(pid) {
  if (!pid) return false;
  if (platform === "win32") {
    const r = spawnSync("tasklist", ["/FI", `PID eq ${pid}`, "/NH"], { encoding: "utf8" });
    return r.stdout.includes(String(pid));
  }
  return spawnSync("kill", ["-0", String(pid)]).status === 0;
}
function hostProcesses() {
  if (platform === "win32") {
    const r = spawnSync("tasklist", ["/FI", "IMAGENAME eq dm-browser-host.exe", "/NH"], {
      encoding: "utf8",
    });
    return (r.stdout.match(/dm-browser-host\.exe/g) ?? []).length;
  }
  const r = spawnSync("pgrep", ["-f", hostExe], { encoding: "utf8" });
  return r.stdout.trim() ? r.stdout.trim().split("\n").length : 0;
}

// ------------------------------------------------------------------ DevTools pipe client
const browsers = [];
const stderrLogFile = path.join(outDir, "browser-stderr.log");
const tail = (text, n = 2000) => (text.length > n ? text.slice(-n) : text);
function sanitize(text) {
  let clean = "";
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (code === 9 || code === 10 || code === 13 || (code >= 32 && code !== 127)) clean += ch;
  }
  return clean.split(work).join("<work>").split(home).join("<home>");
}
function launchBrowser(profile, browserEnv) {
  const args = [
    `--user-data-dir=${profile}`,
    "--remote-debugging-pipe",
    "--enable-unsafe-extension-debugging",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-sync",
    "--disable-features=Translate,MediaRouter",
    // No OS keyring: with a D-Bus session but no Secret Service (the packaged smoke runs under
    // dbus-run-session) Chrome can block at startup waiting for one; macOS would prompt for the
    // keychain.
    ...(platform === "linux" ? ["--password-store=basic"] : []),
    ...(platform === "darwin" ? ["--use-mock-keychain"] : []),
    // Windows Edge otherwise relaunches itself through its compatibility layer, and the relaunched
    // process does not inherit the DevTools pipe.
    ...(kind === "edge" && platform === "win32" ? ["--edge-skip-compat-layer-relaunch"] : []),
    ...(headed ? [] : ["--headless=new"]),
    "about:blank",
  ];
  const child = spawn(browserPath, args, {
    env: browserEnv,
    stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"],
  });
  // Bounded stderr: the reason a browser refuses to start or to answer DevTools is only here.
  const launch = {
    pid: child.pid,
    started_at: new Date().toISOString(),
    exit: null,
    stderr_bytes: 0,
    stderr: Buffer.alloc(0),
  };
  const stderrText = () => sanitize(launch.stderr.toString("utf8"));
  child.stderr.on("data", (chunk) => {
    launch.stderr_bytes += chunk.length;
    launch.stderr = Buffer.concat([launch.stderr, chunk]);
    if (launch.stderr.length > 8192) launch.stderr = launch.stderr.subarray(-8192);
    fs.appendFileSync(stderrLogFile, sanitize(chunk.toString("utf8")));
  });
  const toBrowser = child.stdio[3];
  const fromBrowser = child.stdio[4];
  toBrowser.on("error", () => undefined);
  fromBrowser.on("error", () => undefined);
  let pending = Buffer.alloc(0);
  let nextId = 1;
  const waiting = new Map();
  fromBrowser.on("data", (chunk) => {
    pending = Buffer.concat([pending, chunk]);
    let i;
    while ((i = pending.indexOf(0)) >= 0) {
      const msg = JSON.parse(pending.subarray(0, i).toString("utf8"));
      pending = pending.subarray(i + 1);
      const w = msg.id !== undefined && waiting.get(msg.id);
      if (w) {
        waiting.delete(msg.id);
        if (msg.error) w.reject(new Error(`${w.method}: ${msg.error.message}`));
        else w.resolve(msg.result);
      }
    }
  });
  let exited = false;
  child.on("exit", (code, signal) => {
    exited = true;
    launch.exit = { code, signal, at: new Date().toISOString() };
    fs.appendFileSync(
      stderrLogFile,
      `\n[${label}] browser pid ${launch.pid} exited code=${code} signal=${signal}\n`,
    );
    for (const w of waiting.values())
      w.reject(new Error(`browser exited (code ${code ?? "none"}${signal ? `, ${signal}` : ""})`));
    waiting.clear();
  });
  const send = (method, params = {}, sessionId, timeoutMs = 60_000) =>
    new Promise((resolve, reject) => {
      if (exited) return reject(new Error("browser exited"));
      const id = nextId++;
      // Every call is bounded: a browser that never answers names the stalled method instead of
      // hanging until an outer timeout kills the harness without results (B2.6).
      const timer = setTimeout(() => {
        if (waiting.delete(id))
          reject(
            new Error(`${method}: no answer from the browser in ${Math.round(timeoutMs / 1000)} s`),
          );
      }, timeoutMs);
      const done = (fn) => (v) => {
        clearTimeout(timer);
        fn(v);
      };
      waiting.set(id, { resolve: done(resolve), reject: done(reject), method });
      try {
        toBrowser.write(JSON.stringify({ id, method, params, sessionId }) + "\0");
      } catch (e) {
        clearTimeout(timer);
        waiting.delete(id);
        reject(e);
      }
    });
  const browser = {
    child,
    send,
    exited: () => exited,
    pid: child.pid,
    launch,
    stderrText,
    abort: () => {
      killTree(child.pid);
    },
  };
  browsers.push(browser);
  return browser;
}

async function attach(browser, match) {
  const target = await until(
    "target",
    async () => (await browser.send("Target.getTargets")).targetInfos.find(match),
    20_000,
  );
  const { sessionId } = await browser.send("Target.attachToTarget", {
    targetId: target.targetId,
    flatten: true,
  });
  return sessionId;
}
async function evaluate(browser, sessionId, expression) {
  const r = await browser.send(
    "Runtime.evaluate",
    { expression, awaitPromise: true, returnByValue: true },
    sessionId,
  );
  if (r.exceptionDetails)
    throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
}
async function startBrowser(profile, extDir, first = false) {
  const browser = launchBrowser(profile, browserEnv);
  if (first) {
    let version;
    try {
      version = await browser.send("Browser.getVersion", {}, undefined, 20_000);
    } catch (e) {
      record("0 DevTools transport ready (Browser.getVersion)", "failed", {
        error: String(e?.message ?? e),
        browser_stderr: tail(browser.stderrText()),
        isolated_config_home: results.browser_runtime?.isolated_config_home ?? null,
      });
      throw new Error(`DevTools transport unavailable: ${e?.message ?? e}`);
    }
    record("0 DevTools transport ready (Browser.getVersion)", "passed", {
      product: version.product,
      protocol: version.protocolVersion,
      isolated_config_home: results.browser_runtime?.isolated_config_home ?? null,
    });
  }
  let id;
  try {
    ({ id } = await browser.send("Extensions.loadUnpacked", { path: extDir }));
  } catch (e) {
    throw new Error(`extension loading failed: ${e?.message ?? e}`);
  }
  const worker = await attach(
    browser,
    (t) => t.type === "service_worker" && t.url === `chrome-extension://${id}/background.js`,
  );
  // The worker target can be attached before its module has run.
  await until(
    "the e2e hook in the extension worker (build with --mode e2e)",
    async () => (await evaluate(browser, worker, "typeof downovaE2E").catch(() => "")) === "object",
    15_000,
  );
  return { browser, id, worker };
}
async function closeBrowser(browser) {
  await browser.send("Browser.close", {}, undefined, 2_000).catch(() => undefined);
  await until("browser to exit", () => browser.exited(), 10_000);
}
async function popupText(browser, id, selector, accept) {
  const { targetId } = await browser.send("Target.createTarget", {
    url: `chrome-extension://${id}/popup.html`,
  });
  const session = await attach(browser, (t) => t.targetId === targetId);
  const text = await until(
    `popup ${selector}`,
    async () => {
      const t = await evaluate(
        browser,
        session,
        `document.querySelector(${JSON.stringify(selector)})?.textContent ?? ""`,
      );
      return accept(t) ? t : null;
    },
    20_000,
  );
  await browser.send("Target.closeTarget", { targetId });
  return text;
}
const settled = (browser, worker, recordId) =>
  until(
    "hand-off to settle",
    async () => {
      const list = await evaluate(browser, worker, "downovaE2E.list()");
      const r = recordId ? list.find((x) => x.id === recordId) : list[0];
      return r && r.state !== "sending" ? r : null;
    },
    60_000,
  );

// ------------------------------------------------------------------ registration
const vendor = kind === "edge" ? "Microsoft\\Edge" : "Google\\Chrome";
const regKey = `HKCU\\Software\\${vendor}\\NativeMessagingHosts\\${HOST_NAME}`;
let previousRegistration = null;
let registered = false;
// The product's manifest for this browser, from the installed host's own status.
function productManifest(hostExe) {
  const r = spawnSync(hostExe, ["status", "--browser", kind, "--json"], { encoding: "utf8" });
  let row = {};
  try {
    row = JSON.parse(r.stdout.trim().split("\n")[0]);
  } catch {
    // reported through `code`
  }
  return { code: r.status, row };
}
function register(profile, hostExe) {
  const nmRoot = path.join(work, "nm");
  if (platform === "win32") {
    const q = spawnSync("reg", ["query", regKey, "/ve"], { encoding: "utf8" });
    if (q.status === 0) previousRegistration = /REG_SZ\s+(.+)$/m.exec(q.stdout)?.[1]?.trim() ?? "";
    const r = spawnSync(
      hostExe,
      ["register", "--browser", kind, "--host-path", hostExe, "--root", nmRoot, "--replace"],
      {
        encoding: "utf8",
      },
    );
    registered = r.status === 0;
    return { code: r.status, out: (r.stdout + r.stderr).trim(), previous: previousRegistration };
  }
  // macOS/Linux: Chrome also reads <user-data-dir>/NativeMessagingHosts, so the manifest stays
  // inside the temporary profile.
  const r = spawnSync(
    hostExe,
    ["register", "--browser", kind, "--host-path", hostExe, "--root", nmRoot],
    {
      encoding: "utf8",
    },
  );
  const written = /registered (.+)$/m.exec(r.stdout)?.[1]?.trim();
  const dir = path.join(profile, "NativeMessagingHosts");
  fs.mkdirSync(dir, { recursive: true });
  if (written) fs.copyFileSync(written, path.join(dir, `${HOST_NAME}.json`));
  registered = r.status === 0;
  return { code: r.status, out: (r.stdout + r.stderr).trim() };
}
function unregister(hostExe) {
  if (!registered || platform !== "win32") return null;
  const r = spawnSync(hostExe, ["unregister", "--browser", kind, "--root", path.join(work, "nm")], {
    encoding: "utf8",
  });
  if (previousRegistration !== null)
    spawnSync("reg", ["add", regKey, "/ve", "/t", "REG_SZ", "/d", previousRegistration, "/f"]);
  return { code: r.status, restored_previous: previousRegistration !== null };
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
let service = null;
let current = null;
const hostExe = installedDir
  ? path.join(installedDir, `dm-browser-host${exe}`)
  : path.join(work, "host", `dm-browser-host${exe}`);
const menuUrlArg = arg("--menu-url");
const menuSha = arg("--menu-sha256");
try {
  if (!browserPath) {
    record("browser available", "unavailable", { reason: "no Chrome found; pass --browser PATH" });
    throw new Error("no browser");
  }
  log(`browser ${browserPath} ${browserVersion}; work ${work}; fixture ${base}`);

  // Builds: debug service, CLI and host (unless installed); the extension in e2e mode (exposes
  // the menu listener).
  if (!installedDir) {
    const build = spawnSync(
      "cargo",
      ["build", "--locked", "-p", "dm-service", "-p", "dm-cli", "-p", "dm-browser-host"],
      { cwd: root, stdio: "inherit" },
    );
    if (build.status !== 0) throw new Error("cargo build failed");
  }
  const extDir = path.join(work, "extension");
  const vite = path.join(
    root,
    "apps/extension/node_modules/.bin",
    platform === "win32" ? "vite.cmd" : "vite",
  );
  const vb = spawnSync(vite, ["build", "--mode", "e2e", "--outDir", extDir, "--emptyOutDir"], {
    cwd: path.join(root, "apps/extension"),
    stdio: "inherit",
    shell: platform === "win32",
  });
  if (vb.status !== 0) throw new Error("extension e2e build failed");
  if (!installedDir) {
    // A stable copy of the host outside target/ (never an AppImage mount).
    fs.mkdirSync(path.dirname(hostExe), { recursive: true });
    fs.copyFileSync(bin("dm-browser-host"), hostExe);
    service = spawn(bin("dm-service"), ["--data-dir", home, "--download-dir", dl], {
      env,
      stdio: "ignore",
    });
    await until("service hello", () => dm(["hello"]).code === 0, 30_000);
    record("service started (isolated data and download directories)", "passed", {
      pid: service.pid,
    });
  } else {
    // The packaged desktop started the service; the host never starts one.
    const hello = dm(["hello"]);
    record(
      "installed service already running (started by the desktop)",
      hello.code === 0 ? "passed" : "failed",
      { code: hello.code },
    );
  }

  let profile = path.join(work, "profile");
  if (productRegistration) {
    const { code, row } = productManifest(hostExe);
    const owned =
      row.ok === true &&
      row.result?.owner === "ours" &&
      (platform !== "win32" || row.result?.registry_owner?.owner === "ours");
    record(
      `product registration for ${kind} found and owned by this installation`,
      owned ? "passed" : "failed",
      {
        code,
        manifest: row.result?.manifest,
        owner: row.result?.owner,
        registry_value: row.result?.registry_value ?? null,
      },
    );
    if (!owned) throw new Error("no product registration");
    // macOS/Linux: the browser's default user-data directory is the manifest's grandparent.
    if (platform !== "win32") profile = path.dirname(path.dirname(row.result.manifest));
  } else {
    const reg = register(profile, hostExe);
    record(`host registered for ${kind} (reversible)`, reg.code === 0 ? "passed" : "failed", reg);
  }
  // Chrome 136+ refuses DevTools on the default profile. On Linux product registration the
  // manifest (and therefore the profile) is that default, so give the browser an isolated config
  // home while keeping --user-data-dir on the product directory.
  const runtime = browserRuntimeEnv({
    platform,
    kind,
    productRegistration,
    profile,
    env,
    work,
  });
  browserEnv = runtime.env;
  results.browser_runtime = {
    profile,
    isolated_config_home: runtime.configHome,
    default_profile: runtime.isolated,
  };
  if (runtime.isolated)
    log(
      `profile is this platform's default; browser config home isolated at ${runtime.configHome}`,
    );

  // 1. Identity
  current = await startBrowser(profile, extDir, true);
  record(
    "1 extension loaded; ID equals the host allowlist",
    current.id === DEV_ID ? "passed" : "failed",
    { id: current.id, expected: DEV_ID },
  );

  // 2. Readiness through the popup
  const ready = await popupText(
    current.browser,
    current.id,
    "#service",
    (t) => !t.startsWith("Checking"),
  );
  record(
    "2 popup readiness check: popup → worker → host → service",
    ready.startsWith("Service ready.") ? "passed" : "failed",
    { text: ready },
  );

  // 3. Context-menu hand-off
  const menuUrl = menuUrlArg ?? `${base}/menu.bin`;
  const expectedMenuSha = menuSha ?? sha256(bodies["menu.bin"]);
  const menuName = new URL(menuUrl).pathname.split("/").pop();
  await evaluate(
    current.browser,
    current.worker,
    `downovaE2E.menuClick({ menuItemId: downovaE2E.menuId, linkUrl: ${JSON.stringify(menuUrl)} })`,
  );
  const menu = await settled(current.browser, current.worker);
  const menuDone = menu.jobId ? await finished(menu.jobId) : null;
  const menuJobs = jobs().filter((j) => j.source_display?.includes(`/${menuName}`));
  record(
    "3 simulated context-menu click (worker listener) → real native messaging → host → service; one job; file SHA-256",
    menu.state === "accepted" &&
      menuDone?.phase === "completed" &&
      menuDone.sha256 === expectedMenuSha &&
      menuJobs.length === 1
      ? "passed"
      : "failed",
    {
      state: menu.state,
      job: menu.jobId,
      phase: menuDone?.phase,
      sha256: menuDone?.sha256,
      service_jobs_for_link: menuJobs.length,
    },
  );
  const shown = await popupText(current.browser, current.id, "#handoffs li", (t) => t.length > 0);
  record(
    "3b popup shows acceptance, not completion",
    /Accepted by Downova/.test(shown) && /not necessarily started/.test(shown)
      ? "passed"
      : "failed",
    { text: shown },
  );

  if (handoffOnly) {
    await closeBrowser(current.browser);
    current = null;
    throw Object.assign(new Error("handoff-only"), { done: true });
  }

  // 4. Lost acknowledgement, browser + worker restart, retry with the same key
  const lostUrl = `${base}/lostack.bin`;
  const lost = await evaluate(
    current.browser,
    current.worker,
    `downovaE2E.start(${JSON.stringify(lostUrl)}, 1)`,
  );
  // Let the discarded request reach the service before the browser goes away.
  await until(
    "the lost request to reach the service",
    () => jobs().some((j) => j.display_name === "lostack.bin"),
    20_000,
  );
  await closeBrowser(current.browser);
  current = await startBrowser(profile, extDir);
  const kept = (await evaluate(current.browser, current.worker, "downovaE2E.list()")).find(
    (r) => r.id === lost.id,
  );
  await evaluate(current.browser, current.worker, `downovaE2E.retry(${JSON.stringify(lost.id)})`);
  const retried = await settled(current.browser, current.worker, lost.id);
  const lostJobs = jobs().filter((j) => j.display_name.startsWith("lostack"));
  record(
    "4 lost acknowledgement → uncertain; after a browser/worker restart the retry with the same key gives one job",
    lost.state === "uncertain" &&
      kept?.state === "uncertain" &&
      kept.idempotencyKey === lost.idempotencyKey &&
      retried.state === "accepted" &&
      lostJobs.length === 1 &&
      lostJobs[0].job_id === retried.jobId
      ? "passed"
      : "failed",
    {
      first: lost.state,
      after_restart: kept?.state ?? "missing",
      same_key: kept?.idempotencyKey === lost.idempotencyKey,
      retry: retried.state,
      service_jobs_for_link: lostJobs.length,
    },
  );

  // 5. Close the browser after acceptance
  const slowUrl = `${base}/slow.bin?slow=80`;
  const slow = await evaluate(
    current.browser,
    current.worker,
    `downovaE2E.start(${JSON.stringify(slowUrl)})`,
  );
  // Close only once bytes are flowing, so the browser goes away in the middle of the transfer.
  const before = await until(
    "the slow transfer to be under way",
    () => {
      const g = dm(["get", slow.jobId ?? ""]).json?.data;
      return g?.phase === "transferring" && Number(g.downloaded_bytes) > 0 ? g : null;
    },
    30_000,
  );
  await closeBrowser(current.browser);
  await sleep(1000);
  const hostsLeft = hostProcesses();
  const during = dm(["get", slow.jobId ?? ""]).json?.data;
  const slowDone = slow.jobId ? await finished(slow.jobId) : null;
  record(
    "5 browser closed after acceptance; service keeps transferring; SHA-256",
    slow.state === "accepted" &&
      Number(before.downloaded_bytes) < bodies["slow.bin"].length &&
      alive(service.pid) &&
      hostsLeft === 0 &&
      slowDone?.phase === "completed" &&
      slowDone.sha256 === sha256(bodies["slow.bin"])
      ? "passed"
      : "failed",
    {
      state: slow.state,
      phase_at_close: before?.phase,
      bytes_at_close: before?.downloaded_bytes,
      phase_after_close: during?.phase,
      host_processes_after_close: hostsLeft,
      final: slowDone,
    },
  );
  current = null;
} catch (e) {
  if (!e?.done) record("e2e run", "failed", { error: String(e?.message ?? e) });
} finally {
  for (const browser of browsers) {
    if (!browser.exited()) await closeBrowser(browser).catch(() => browser.abort());
  }
  current = null;
  const un = !productRegistration && fs.existsSync(hostExe) ? unregister(hostExe) : null;
  if (un)
    record(
      "host unregistered (previous value restored if any)",
      un.code === 0 ? "passed" : "failed",
      un,
    );
  if (!installedDir) killTree(service?.pid);
  server.close();
  results.finished_at = new Date().toISOString();
  const failed = results.steps.filter((s) => s.status === "failed");
  results.kind = kind;
  results.browser_diagnostics = {
    stderr_log: "browser-stderr.log",
    launches: browsers.map((b) => ({
      pid: b.launch.pid,
      started_at: b.launch.started_at,
      exit: b.launch.exit,
      stderr_bytes: b.launch.stderr_bytes,
      stderr: b.stderrText(),
    })),
  };
  results.verdict = failed.length
    ? "failed"
    : results.steps.some((s) => s.status === "unavailable")
      ? "unavailable"
      : "passed";
  exitCode = failed.length ? 1 : 0;
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2));
  fs.writeFileSync(path.join(outDir, "e2e.log"), logLines.join("\n") + "\n");
  fs.writeFileSync(
    stderrLogFile,
    browsers
      .map((b, i) => `# launch ${i + 1} pid ${b.launch.pid}\n${b.stderrText()}\n`)
      .join("\n") + "\n",
  );
  publishRows(`Real-browser hand-off ${label} ${browserVersion ?? ""}`, results);
  console.log(`\nverdict: ${results.verdict} (${outDir})`);
  try {
    fs.rmSync(work, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
  } catch (e) {
    console.log(`could not remove ${work}: ${e.code}`);
  }
  process.exit(exitCode);
}
