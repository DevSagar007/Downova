#!/usr/bin/env node
// CI tool-cache integrity for Tauri's Windows bundling tools (%LOCALAPPDATA%\tauri: NSIS, WiX).
//
//   node scripts/lib/tool-cache.mjs snapshot <dir>   # after a successful build: record digests
//   node scripts/lib/tool-cache.mjs validate <dir>   # after a cache restore: drop bad entries
//
// `snapshot` writes <dir>/.downova-tools.json: the SHA-256 of every file, grouped by top-level
// entry: a folder (NSIS, WixTools314 on Windows) or a single file (AppRun-x86_64 and the linuxdeploy
// AppImages in Linux's ~/.cache/tauri). `validate` re-hashes a restored folder against it and deletes
// only the top-level entries that are missing files, changed, or carry unrecorded files, so Tauri
// downloads (and verifies) just those again. Without a manifest nothing restored is trusted: the
// whole folder is removed. The tools were verified by Tauri when first downloaded; this keeps a
// corrupted or tampered cache from being reused because its file names exist.
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MANIFEST = ".downova-tools.json";

const digest = (p) => createHash("sha256").update(fs.readFileSync(p)).digest("hex");
/** What a top-level entry holds now: a folder's files, or a single file's digest. */
const current = (p, isDir) => (isDir ? walk(p) : { ".": digest(p) });

function walk(dir, base = dir, out = {}) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, base, out);
    else if (e.isFile())
      out[path.relative(base, p).replaceAll("\\", "/")] = createHash("sha256")
        .update(fs.readFileSync(p))
        .digest("hex");
  }
  return out;
}

export function snapshot(dir) {
  const entries = {};
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === MANIFEST) continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) entries[e.name] = walk(p);
    else if (e.isFile()) entries[e.name] = { ".": digest(p) };
  }
  fs.writeFileSync(path.join(dir, MANIFEST), JSON.stringify({ entries }, null, 1) + "\n");
  return entries;
}

/** Removes untrusted entries; returns { kept, removed }. */
export function validate(dir) {
  const result = { kept: [], removed: [] };
  if (!fs.existsSync(dir)) return result;
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(path.join(dir, MANIFEST), "utf8")).entries;
  } catch {
    manifest = null;
  }
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === MANIFEST) continue;
    const p = path.join(dir, e.name);
    const recorded = manifest?.[e.name];
    const ok =
      (e.isDirectory() || e.isFile()) &&
      recorded &&
      JSON.stringify(Object.entries(current(p, e.isDirectory())).sort()) ===
        JSON.stringify(Object.entries(recorded).sort());
    if (ok) result.kept.push(e.name);
    else {
      fs.rmSync(p, { recursive: true, force: true });
      result.removed.push(e.name);
    }
  }
  return result;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [cmd, dir] = process.argv.slice(2);
  if (!dir || !["snapshot", "validate"].includes(cmd)) {
    console.error("usage: tool-cache.mjs snapshot|validate <dir>");
    process.exit(2);
  }
  if (cmd === "snapshot") {
    if (!fs.existsSync(dir)) {
      console.log(`tool-cache: ${dir} does not exist; nothing to record`);
      process.exit(0);
    }
    console.log(`tool-cache: recorded ${Object.keys(snapshot(dir)).join(", ") || "nothing"}`);
  } else {
    const r = validate(dir);
    console.log(`tool-cache: kept [${r.kept.join(", ")}], removed [${r.removed.join(", ")}]`);
  }
}
