#!/usr/bin/env node
// B3.2 packaged exercise of F07/E06 (Windows attachment check) and E05 (diagnostics).
//
//   node scripts/acceptance/safety-diagnostics-acceptance.mjs --package-dir dist-packages/windows-x86_64
//
// Windows only. The NORMAL package's MSI is extracted with `msiexec /a` (nothing is installed); the
// packaged desktop starts its packaged service; the real window is driven over WebView2 DevTools;
// `dm` and the fixture (own process) are independent observers. Real Windows Attachment Services
// runs on real files; no antivirus block is provoked here (a human/VM row, W2).
//
// Rows:
//   P1 a download completes only after the attachment check: `scan.status = attachment_checked`,
//      SHA-256 exact, and the file's Zone.Identifier holds ZoneId=3 and only the sanitized origin
//      (no userinfo, path, query or fragment from a URL seeded with synthetic secrets).
//   P2 its details in the real window say "Checked by Windows Attachment Services … not a malware
//      verdict" (English) and the Bangla title after switching language.
//   P3 Help → Diagnostics… in the real window previews the report; no synthetic secret, host or
//      path appears; the crash section is off until switched on.
//   P4 `dm diagnostics-export` writes the previewed snapshot atomically; the file equals the
//      preview, holds no secret, and an existing file is kept without --replace.
//   UI save dialog: NOT RUN (a native dialog DevTools cannot drive; covered by unit tests).
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
  arg("--out", path.join("acceptance-results", `${buildInfo.label}-safety-diagnostics`)),
);
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const SECRET = "SYNTH-SECRET-b32";
const results = {
  started_at: new Date().toISOString(),
  observer: "automated: packaged window over WebView2 DevTools, plus dm CLI and fixture",
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ ...detail, step, status });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 600)}`);
};
const finish = (code) => {
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.some((s) => !["passed", "not run"].includes(s.status))
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
  source: buildInfo.source,
  built_at: buildInfo.built_at,
  signing: buildInfo.signing,
  msi,
  msi_sha256: sha256(fs.readFileSync(path.join(packageDir, msi))),
};

const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-b32-"));
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
const port = 9472;
const env = {
  ...process.env,
  DOWNOVA_HOME: home,
  DOWNOVA_DOWNLOAD_DIR: dl,
  WEBVIEW2_USER_DATA_FOLDER: path.join(work, "webview"),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
};
function dm(args) {
  const r = spawnSync(dmExe, ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
    timeout: 60_000,
  });
  try {
    return { code: r.status, json: JSON.parse(r.stdout) };
  } catch {
    return { code: r.status, json: null, stdout: r.stdout, stderr: r.stderr };
  }
}
const servicePid = () => {
  try {
    return JSON.parse(fs.readFileSync(path.join(home, "run", "endpoint.json"), "utf8")).pid;
  } catch {
    return null;
  }
};
const kill = (pid) => pid && spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"]);
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const fixture = spawn(
  process.execPath,
  [
    path.join(import.meta.dirname, "limits-schedules-fixture.mjs"),
    `setup.exe:${4 * 1024 * 1024}:31`,
  ],
  { stdio: ["ignore", "pipe", "inherit"] },
);
const { base, sha256: expected } = await readFirstJsonLine(fixture, { timeoutMs: 60_000 });
const host = new URL(base).host;
// Userinfo, a path token, a query and a fragment, all carrying the synthetic secret.
const secretUrl = `http://user:${SECRET}@${host}/setup.exe?token=${SECRET}#${SECRET}`;

const desktop = spawn(desktopExe, [], { env, stdio: "ignore" });
let page = null;
const shot = async (name) => {
  fs.writeFileSync(path.join(outDir, name), await page.screenshot());
  return name;
};
try {
  await until("the service", () => dm(["hello"]).code === 0, 60_000);
  page = await devtools(port);
  await page.ui(`until(() => document.querySelector("#download-list"), 30000).then(() => true)`);

  // P1: the real attachment check before completion.
  const added = dm(["add", secretUrl]);
  const id = added.json?.data?.job_id;
  const phases = new Set();
  const done = await until(
    "the download to finish",
    () => {
      const j = dm(["get", id]).json?.data;
      if (j) phases.add(j.phase);
      return j && ["completed", "failed", "cancelled"].includes(j.phase) ? j : null;
    },
    120_000,
  );
  const file = path.join(dl, done.display_name);
  let zone = null;
  try {
    zone = fs.readFileSync(`${file}:Zone.Identifier`, "utf8").replace(/\0/g, "");
  } catch (e) {
    zone = `unreadable: ${e.code}`;
  }
  const exact =
    done.phase === "completed" && sha256(fs.readFileSync(file)) === expected["setup.exe"];
  record(
    "P1 the packaged service completes a download only after the real Windows attachment check; Zone.Identifier carries only the sanitized origin",
    exact &&
      done.scan?.status === "attachment_checked" &&
      done.scan?.origin_marking === "present" &&
      /ZoneId=3/.test(zone) &&
      zone.includes(`HostUrl=http://${host}/`) &&
      !zone.includes(SECRET) &&
      !zone.includes("setup.exe")
      ? "passed"
      : "failed",
    { phases_seen: [...phases], scan: done.scan, zone_identifier: zone, sha256_exact: exact },
  );

  // P2: the details in the real window, then Bangla.
  const p2 = await page.ui(`(async () => {
    const row = await until(() => [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes("setup.exe")), 15000);
    row.click();
    const title = await until(() => byText("h2", /^Checked by Windows Attachment Services$/), 15000);
    return { note: title.closest("section")?.textContent ?? null, row: row.textContent };
  })()`);
  await shot("p2-details-checked-en.png");
  const p2bn = await page.ui(`(async () => {
    click(byText("button", /^Options$/));
    const lang = await until(() => labelled("Language"), 10000);
    setValue(lang, "bn");
    await until(() => labelled("ভাষা"), 5000);
    click([...document.querySelectorAll("[role=dialog] button")].find((b) => /^(ঠিক আছে|বন্ধ করুন)$/.test((b.textContent || b.getAttribute("aria-label") || "").trim())));
    const title = await until(() => byText("h2", /^Windows Attachment Services যাচাই করেছে$/), 10000);
    return { title: title.textContent };
  })()`);
  await shot("p2-details-checked-bn.png");
  await page.ui(`(async () => {
    click(byText("button", /^অপশন$/));
    setValue(await until(() => labelled("ভাষা"), 10000), "en");
    await until(() => labelled("Language"), 5000);
    click([...document.querySelectorAll("[role=dialog] button")].find((b) => /^(Done|Close)$/.test((b.textContent || b.getAttribute("aria-label") || "").trim())));
    return true;
  })()`);
  record(
    "P2 the real window explains the check without a safety claim (English and Bangla)",
    /not a malware verdict/.test(p2.note ?? "") &&
      /Mark of the Web is on the file/.test(p2.note ?? "") &&
      !/malware-free/i.test(p2.note ?? "") &&
      /Windows Attachment Services/.test(p2bn.title ?? "")
      ? "passed"
      : "failed",
    { note: p2.note, bangla_title: p2bn.title },
  );

  // P3: Help → Diagnostics… preview in the real window.
  const p3 = await page.ui(`(async () => {
    click(byText("[role=menuitem]", /^Help$/));
    click(await until(() => byText("[role=menuitem]", /^Diagnostics/), 5000));
    const box = await until(() => [...document.querySelectorAll("textarea")].find((x) => x.getAttribute("aria-label") === "Diagnostics preview" && x.value), 15000);
    const first = box.value;
    const crash = labelled("Include the last startup's recovery summary (crash-related)");
    crash.click();
    await until(() => box.value.includes('"crash"'), 10000);
    const second = box.value;
    return { first, second };
  })()`);
  await shot("p3-diagnostics-preview-en.png");
  const leaked = (s) =>
    [SECRET, host, base, dl, home, os.userInfo().username].filter((n) => n && s.includes(n));
  const p3json = JSON.parse(p3.second);
  record(
    "P3 Help → Diagnostics… previews the redacted report in the real window; crash section off by default; no secret, host, path or user name",
    !p3.first.includes('"crash"') &&
      p3.second.includes('"crash"') &&
      leaked(p3.first).length === 0 &&
      leaked(p3.second).length === 0 &&
      p3json.platform?.attachment_check === "windows_attachment_services" &&
      p3json.jobs?.scan_by_status?.attachment_checked === 1
      ? "passed"
      : "failed",
    {
      leaked: [...leaked(p3.first), ...leaked(p3.second)],
      platform: p3json.platform,
      jobs: p3json.jobs,
    },
  );
  await page.ui(
    `(async () => { click([...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.trim() === "Done")); return true; })()`,
  );

  // P4: the CLI export (the same service implementation).
  const out = path.join(work, "support", "downova-diagnostics.json");
  fs.mkdirSync(path.dirname(out), { recursive: true });
  const preview = dm(["diagnostics"]);
  const exported = dm(["diagnostics-export", out]);
  const written = fs.existsSync(out) ? fs.readFileSync(out, "utf8") : "";
  const again = dm(["diagnostics-export", out]);
  const replaced = dm(["diagnostics-export", out, "--replace"]);
  const leftovers = fs
    .readdirSync(path.dirname(out))
    .filter((n) => n !== "downova-diagnostics.json");
  record(
    "P4 dm diagnostics-export writes the redacted report atomically; an existing file is kept without --replace; no temporary file remains",
    preview.code === 0 &&
      exported.code === 0 &&
      written.length > 0 &&
      sha256(Buffer.from(written)) === exported.json?.data?.sha256 &&
      leaked(written).length === 0 &&
      again.code !== 0 &&
      replaced.code === 0 &&
      leftovers.length === 0
      ? "passed"
      : "failed",
    {
      export_exit: exported.code,
      size: exported.json?.data?.size_bytes,
      second_without_replace_exit: again.code,
      second_error: again.json?.error?.kind,
      replace_exit: replaced.code,
      leaked: leaked(written),
      leftovers,
    },
  );
  record("UI save dialog", "not run", {
    reason:
      "the native save dialog cannot be driven over DevTools; the shell path is unit-tested and the export path is P4",
  });
} catch (e) {
  record("run", "failed", { error: String(e?.stack ?? e) });
  try {
    await shot("failure.png");
  } catch {
    /* no page */
  }
} finally {
  page?.close();
  kill(desktop.pid);
  kill(servicePid());
  fixture.kill();
  await sleep(1500);
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    /* a just-ended process may still hold a file */
  }
}
record("installed app", "not run", {
  reason: "msiexec /a only; installed-app rows W1–W4, D1–D2 are human checks",
});
finish();
