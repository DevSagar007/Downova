#!/usr/bin/env node
// Fetches the pinned aria2 Windows build into tools/aria2/ and verifies it (REL-05).
// macOS/Linux use the system package (brew / apt) — the adapter reports the actual version.
//
// Pin provenance: the release asset publishes no digest (GitHub API `digest: null`); the SHA-256
// below was recorded on first download (2026-10-21) and cross-checked against the API size.
//
// Reliability: the download is retried within finite bounds on transient failures only
// (scripts/lib/download.mjs); a size or SHA-256 mismatch is never retried. The install is staged
// and each file is moved into place only after extraction succeeded, and `.pin.json` records what
// was installed: a later run reuses tools/aria2 only if aria2c.exe still has the recorded digest
// (a partial or corrupt earlier install is replaced, not trusted because its name exists).
import { createHash } from "node:crypto";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { downloadVerified } from "./lib/download.mjs";

const PIN = {
  version: "1.37.0",
  url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
  size: 2475379,
  sha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
  folder: "aria2-1.37.0-win-64bit-build1",
};
const FILES = ["aria2c.exe", "COPYING", "LICENSE.OpenSSL", "AUTHORS"];

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dest = path.join(root, "tools", "aria2");
const marker = path.join(dest, ".pin.json");
const fileSha = (p) => createHash("sha256").update(fs.readFileSync(p)).digest("hex");

/** Whether tools/aria2 holds exactly what this pin installs (verified, not just present). */
export function installedMatches(dir = dest) {
  try {
    const m = JSON.parse(fs.readFileSync(path.join(dir, ".pin.json"), "utf8"));
    return (
      m.zip_sha256 === PIN.sha256 &&
      FILES.every((f) => m.files?.[f] && fileSha(path.join(dir, f)) === m.files[f])
    );
  } catch {
    return false;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.platform !== "win32") {
    console.error(
      "fetch-aria2: Windows only. Install aria2 with your package manager (brew install aria2 / apt-get install aria2).",
    );
    process.exit(1);
  }
  if (!process.argv.includes("--force") && installedMatches()) {
    console.log(`fetch-aria2: ${path.relative(root, dest)} already holds the verified pin`);
    process.exit(0);
  }
  if (fs.existsSync(path.join(dest, "aria2c.exe")))
    console.log("fetch-aria2: the existing install does not match its recorded pin; replacing it");

  let bytes;
  try {
    bytes = await downloadVerified(PIN.url, { tool: "aria2", size: PIN.size, sha256: PIN.sha256 });
  } catch (e) {
    console.error(`fetch-aria2: ${e.message}`);
    process.exit(1);
  }

  fs.mkdirSync(dest, { recursive: true });
  const staging = path.join(dest, `.staging-${process.pid}`);
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging);
  try {
    const zip = path.join(staging, "aria2.zip");
    fs.writeFileSync(zip, bytes);
    // Windows' bundled bsdtar extracts zip archives.
    execFileSync(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe"), [
      "-xf",
      zip,
      "-C",
      staging,
    ]);
    const digests = {};
    for (const f of FILES) {
      const from = path.join(staging, PIN.folder, f);
      if (!fs.existsSync(from)) throw new Error(`the archive lacks ${f}`);
      digests[f] = fileSha(from);
    }
    // Publish: the marker goes first (so a crash mid-publish can never look verified), then each
    // file is renamed into place, then the marker is written for the complete set.
    fs.rmSync(marker, { force: true });
    for (const f of FILES) fs.renameSync(path.join(staging, PIN.folder, f), path.join(dest, f));
    fs.writeFileSync(
      marker,
      JSON.stringify({ version: PIN.version, zip_sha256: PIN.sha256, files: digests }, null, 2) +
        "\n",
    );
  } catch (e) {
    console.error(`fetch-aria2: install failed: ${e.message}`);
    process.exit(1);
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
  console.log(
    `fetch-aria2: installed aria2 ${PIN.version} (sha256 verified) to ${path.relative(root, dest)}`,
  );
}
