#!/usr/bin/env node
// Store-ready browser extension packages (v1.0 release preparation).
//   node scripts/store/pack-extensions.mjs        → dist-store/
//     downova-chromium-<version>.zip   upload to the Chrome Web Store and to Edge Add-ons
//     downova-firefox-<version>.zip    upload to addons.mozilla.org (AMO) for signing
//     SHA256SUMS.txt
// The builds are the normal ones (no e2e hooks, capture permissions optional) in the `store`
// flavour: the Chromium manifest has no `key` (each store assigns its own item ID) and the Firefox
// manifest carries the distributed add-on ID. Nothing is uploaded or signed here.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { entriesFromDir, writeZip } from "../lib/zip.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ext = path.join(root, "apps/extension");
const out = path.join(root, "dist-store");
const FIREFOX_ADDON_ID = "{20566e36-cebf-40ec-960b-160afd8c6ded}";

function fail(why) {
  console.error(`store: ${why}`);
  process.exit(1);
}

function build(mode, dir) {
  const vite = path.join(ext, "node_modules/vite/bin/vite.js");
  const r = spawnSync(process.execPath, [vite, "build", "--mode", mode], {
    cwd: ext,
    stdio: "inherit",
  });
  if (r.status !== 0) fail(`vite build --mode ${mode} failed`);
  const built = path.join(ext, dir);
  const manifest = JSON.parse(fs.readFileSync(path.join(built, "manifest.json"), "utf8"));
  const js = fs
    .readdirSync(built, { recursive: true })
    .map(String)
    .filter((n) => n.endsWith(".js"));
  if (js.some((n) => fs.readFileSync(path.join(built, n), "utf8").includes("downovaE2E")))
    fail(`the ${mode} build contains the e2e hook`);
  if (fs.readdirSync(built, { recursive: true }).some((n) => String(n).endsWith(".map")))
    fail(`the ${mode} build contains source maps`);
  return { built, manifest };
}

const chromium = build("store", "dist-store");
const m = chromium.manifest;
if ("key" in m) fail("the Chromium store manifest has a key");
const upFront = [...(m.permissions ?? []), ...(m.host_permissions ?? [])];
if (upFront.some((p) => ["downloads", "webRequest", "cookies"].includes(p) || p.includes("://")))
  fail("the Chromium store manifest grants capture or cookie permissions up front");

const firefox = build("firefox-store", "dist-firefox-store");
const f = firefox.manifest;
if (f.browser_specific_settings?.gecko?.id !== FIREFOX_ADDON_ID)
  fail("the Firefox store manifest does not carry the distributed add-on ID");
if (JSON.stringify(f).match(/"(cookies|downloads|webRequest|optional_permissions)"/))
  fail("the Firefox store manifest asks for a permission the Firefox build does not use");
if (m.version !== f.version) fail("the two store builds disagree on the version");

fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
const sums = [];
for (const [name, dir] of [
  [`downova-chromium-${m.version}.zip`, chromium.built],
  [`downova-firefox-${f.version}.zip`, firefox.built],
]) {
  // Stores expect manifest.json at the root of the archive.
  const entries = entriesFromDir(dir, "").map((e) => ({ ...e, name: e.name.replace(/^\//, "") }));
  if (!entries.some((e) => e.name === "manifest.json"))
    fail(`${name}: no manifest.json at the root`);
  const file = path.join(out, name);
  writeZip(file, entries);
  sums.push(`${createHash("sha256").update(fs.readFileSync(file)).digest("hex")}  ${name}`);
  console.log(`store: ${path.relative(root, file)} (${entries.length} files)`);
}
fs.writeFileSync(path.join(out, "SHA256SUMS.txt"), sums.join("\n") + "\n");
console.log(sums.join("\n"));
