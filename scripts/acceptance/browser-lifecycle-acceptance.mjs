#!/usr/bin/env node
// B3.4 packaged exercise of BRW-07/08/09 (ADR 0029) on the NORMAL Windows package.
//
//   node scripts/acceptance/browser-lifecycle-acceptance.mjs --package-dir dist-packages/windows-x86_64
//
// Windows only. The MSI is extracted twice with `msiexec /a` (nothing is installed): "A" stands for
// this installation and "B" for another live Downova installation. Every registration goes to a
// redirected file root and a private registry subtree (HKCU\Software\Downova-Test-<n>, deleted
// afterwards), so the user's real browser registrations are never touched.
//
// Rows:
//   B1 the package carries the Firefox build: an unsigned .xpi and an unpacked copy with the
//      development add-on ID the packaged host allows; no capture, cookie or test-hook content
//   B2 initial registration by the packaged host for Chrome, Edge and Firefox: Firefox gets its
//      own manifest (allowed_extensions) and its own Mozilla registry key; Chromium keeps
//      allowed_origins; neither overwrites the other
//   B3 repair is repeatable (same entries, owner stays "ours")
//   B4 another live installation's entry is refused without --replace and kept by unregister
//   B5 an "update" in place (the host rewritten at the same path) keeps the registration valid
//   B6 uninstall order: removing A removes only A's entries; B's stay intact; a second removal
//      is a no-op
//   B7 status reports each browser's package and support verdict on this machine (no paths)
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { readZip } from "../lib/zip.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(arg("--package-dir", "dist-packages/windows-x86_64"));
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
const outDir = path.resolve(
  arg("--out", path.join("acceptance-results", `${buildInfo.label}-browser-lifecycle`)),
);
fs.mkdirSync(outDir, { recursive: true });
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const FIREFOX_ID = "downova-dev@extension.downova.invalid";
const CHROME_ORIGIN = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";
const results = {
  started_at: new Date().toISOString(),
  observer: "automated: packaged dm-browser-host from two extracted copies of the MSI",
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ ...detail, step, status });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 500)}`);
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "this script drives Windows packages only" });
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  process.exit(2);
}
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
results.package = {
  source: buildInfo.source,
  built_at: buildInfo.built_at,
  msi,
  msi_sha256: sha256(fs.readFileSync(path.join(packageDir, msi))),
  extension_firefox: buildInfo.extension_firefox,
};
const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-b34-"));
function extract(name) {
  const target = path.join(work, name);
  const ex = spawnSync("msiexec", ["/a", path.join(packageDir, msi), "/qn", `TARGETDIR=${target}`]);
  if (ex.status !== 0) throw new Error(`msiexec /a exited ${ex.status}`);
  const find = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) {
        const f = find(p);
        if (f) return f;
      } else if (e.name.toLowerCase() === "dm-browser-host.exe") return p;
    }
    return null;
  };
  return find(target);
}
const regRoot = String.raw`HKCU\Software\Downova-Test-B34-${process.pid}`;
const fileRoot = path.join(work, "local-appdata");
function host(exe, args) {
  const r = spawnSync(exe, [...args, "--json", "--root", fileRoot, "--registry-root", regRoot], {
    encoding: "utf8",
  });
  const rows = r.stdout
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch {
        return null;
      }
    })
    .filter(Boolean);
  return { code: r.status, rows, stderr: r.stderr };
}
const by = (rows, b) => rows.find((r) => r.browser === b);
const regValue = (key) => {
  const q = spawnSync("reg", ["query", key, "/ve"], { encoding: "utf8" });
  return q.status === 0 ? (/REG_SZ\s+(.+)$/m.exec(q.stdout)?.[1]?.trim() ?? null) : null;
};
const browsers = ["--browser", "chrome", "--browser", "edge", "--browser", "firefox"];

try {
  // B1
  const xpiName = buildInfo.extension_firefox?.xpi;
  const xpi = xpiName ? readZip(path.join(packageDir, xpiName)) : [];
  const names = xpi.map((e) => e.name);
  const xpiManifest = JSON.parse(
    xpi.find((e) => e.name === "manifest.json")?.data.toString("utf8") ?? "{}",
  );
  const leaked = xpi.some(
    (e) => e.name.endsWith(".js") && e.data.toString("utf8").includes("downovaE2E"),
  );
  record(
    "B1 the package carries the Firefox build: an unsigned .xpi with the development add-on ID, no capture/cookie permission, no test hook, no reconnect page",
    xpiManifest.browser_specific_settings?.gecko?.id === FIREFOX_ID &&
      fs.existsSync(path.join(packageDir, "extension-firefox-unpacked", "manifest.json")) &&
      !JSON.stringify(xpiManifest).match(/"(cookies|downloads|webRequest)"/) &&
      !leaked &&
      !names.some((n) => n.startsWith("reconnect")) &&
      names.includes("background.js")
      ? "passed"
      : "failed",
    {
      xpi: xpiName,
      id: xpiManifest.browser_specific_settings?.gecko?.id,
      background: xpiManifest.background,
      files: names.length,
    },
  );

  const hostA = extract("A");
  const hostB = extract("B");
  // B2
  const reg = host(hostA, ["register", ...browsers]);
  const ff = by(reg.rows, "firefox")?.result;
  const ch = by(reg.rows, "chrome")?.result;
  const ffManifest = ff ? JSON.parse(fs.readFileSync(ff.manifest, "utf8")) : {};
  const chManifest = ch ? JSON.parse(fs.readFileSync(ch.manifest, "utf8")) : {};
  record(
    "B2 initial registration by the packaged host: Firefox has its own manifest (allowed_extensions) and Mozilla key; Chromium keeps allowed_origins",
    reg.code === 0 &&
      ff?.manifest !== ch?.manifest &&
      JSON.stringify(ffManifest.allowed_extensions) === JSON.stringify([FIREFOX_ID]) &&
      ffManifest.allowed_origins === undefined &&
      JSON.stringify(chManifest.allowed_origins) === JSON.stringify([CHROME_ORIGIN]) &&
      ff?.registry_key?.endsWith(String.raw`\Mozilla\NativeMessagingHosts\com.downova.host`) &&
      regValue(ff.registry_key) === ff.manifest
      ? "passed"
      : "failed",
    {
      code: reg.code,
      firefox_key: ff?.registry_key?.replace(regRoot, "<test-root>"),
      firefox_allowed_extensions: ffManifest.allowed_extensions,
      chrome_allowed_origins: chManifest.allowed_origins,
    },
  );
  // B3
  const rep1 = host(hostA, ["repair", ...browsers]);
  const rep2 = host(hostA, ["repair", ...browsers]);
  const st = host(hostA, ["status", ...browsers]);
  record(
    "B3 repair is repeatable; every entry stays this installation's",
    rep1.code === 0 &&
      rep2.code === 0 &&
      st.rows.every((r) => r.result?.owner === "ours" && r.result?.registry_owner?.owner === "ours")
      ? "passed"
      : "failed",
    { owners: st.rows.map((r) => [r.browser, r.result?.owner, r.result?.registry_owner?.owner]) },
  );
  // B4
  const takeover = host(hostB, ["register", "--browser", "firefox"]);
  const keptByB = host(hostB, ["unregister", "--browser", "firefox"]);
  const stillA = by(host(hostA, ["status", "--browser", "firefox"]).rows, "firefox")?.result;
  record(
    "B4 another live installation's Firefox entry is refused without --replace and kept by its unregister",
    takeover.code !== 0 &&
      (by(keptByB.rows, "firefox")?.result?.removed ?? []).length === 0 &&
      stillA?.owner === "ours"
      ? "passed"
      : "failed",
    {
      register_exit: takeover.code,
      unregister_kept: by(keptByB.rows, "firefox")?.result?.kept?.length,
      owner_after: stillA?.owner,
    },
  );
  // B5: an in-place update rewrites the host at the same path.
  fs.copyFileSync(hostB, hostA);
  const afterUpdate = by(host(hostA, ["status", "--browser", "firefox"]).rows, "firefox")?.result;
  record(
    "B5 an update in place (the host rewritten at the same path) keeps the Firefox registration valid",
    afterUpdate?.owner === "ours" && afterUpdate?.registry_owner?.owner === "ours"
      ? "passed"
      : "failed",
    { owner: afterUpdate?.owner },
  );
  // B6: uninstall order. B registers Chrome with --replace (a newer install); A uninstalls.
  const bChrome = host(hostB, ["register", "--browser", "chrome", "--replace"]);
  const removeA = host(hostA, ["unregister", ...browsers]);
  const removedA = by(removeA.rows, "firefox")?.result?.removed ?? [];
  const keptChrome = by(removeA.rows, "chrome")?.result?.kept ?? [];
  const chromeOwnerForB = by(host(hostB, ["status", "--browser", "chrome"]).rows, "chrome")?.result;
  const again = host(hostA, ["unregister", ...browsers]);
  record(
    "B6 removal deletes only this installation's entries (Firefox gone; Chrome kept for the other installation); a second removal is a no-op",
    bChrome.code === 0 &&
      removedA.length === 2 &&
      keptChrome.length > 0 &&
      chromeOwnerForB?.owner === "ours" &&
      regValue(ff.registry_key) === null &&
      again.rows.every((r) => (r.result?.removed ?? []).length === 0)
      ? "passed"
      : "failed",
    {
      firefox_removed: removedA.length,
      chrome_kept: keptChrome.length,
      chrome_owner_for_b: chromeOwnerForB?.owner,
    },
  );
  // B7
  const real = spawnSync(hostB, ["status", "--browser", "all", "--json"], { encoding: "utf8" });
  const rows = real.stdout
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  const verdicts = Object.fromEntries(
    rows.map((r) => [
      r.browser,
      (r.support ?? []).map((v) => `${v.package}:${v.support}:${v.reason}`),
    ]),
  );
  record(
    "B7 status reports each browser's package and support verdict on this machine (categories and codes only)",
    rows.length >= 7 &&
      rows.every((r) => Array.isArray(r.packages) && Array.isArray(r.support)) &&
      !JSON.stringify(rows.map((r) => [r.packages, r.support])).match(/[A-Z]:\\|\\Users\\/)
      ? "passed"
      : "failed",
    { verdicts },
  );
} catch (e) {
  record("run", "failed", { error: String(e?.message ?? e) });
} finally {
  spawnSync("reg", ["delete", regRoot, "/f"]);
  results.registry_test_root_removed = regValue(regRoot) === null;
}
results.finished_at = new Date().toISOString();
results.verdict = results.steps.every((s) => ["passed", "not run"].includes(s.status))
  ? "passed"
  : "failed";
fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
console.log(`verdict ${results.verdict}; results in ${outDir}`);
process.exit(results.verdict === "passed" ? 0 : 1);
