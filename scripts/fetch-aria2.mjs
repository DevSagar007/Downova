#!/usr/bin/env node
// Fetches the pinned aria2 Windows build into tools/aria2/ and verifies it (REL-05).
// macOS/Linux use the system package (brew / apt) — the adapter reports the actual version.
//
// Pin provenance: the release asset publishes no digest (GitHub API `digest: null`); the SHA-256
// below was recorded on first download (2026-10-21) and cross-checked against the API size.
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync, copyFileSync, rmSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PIN = {
  version: "1.37.0",
  url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
  size: 2475379,
  sha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
  folder: "aria2-1.37.0-win-64bit-build1",
};

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dest = path.join(root, "tools", "aria2");

if (process.platform !== "win32") {
  console.error(
    "fetch-aria2: Windows only. Install aria2 with your package manager (brew install aria2 / apt-get install aria2).",
  );
  process.exit(1);
}
if (existsSync(path.join(dest, "aria2c.exe")) && !process.argv.includes("--force")) {
  console.log(
    `fetch-aria2: ${path.relative(root, dest)}\\aria2c.exe already present (use --force to refetch)`,
  );
  process.exit(0);
}

const response = await fetch(PIN.url);
if (!response.ok) throw new Error(`download failed: HTTP ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
const digest = createHash("sha256").update(bytes).digest("hex");
if (bytes.length !== PIN.size || digest !== PIN.sha256) {
  console.error(
    `fetch-aria2: checksum mismatch (size ${bytes.length}, sha256 ${digest}) — refusing to install`,
  );
  process.exit(1);
}

mkdirSync(dest, { recursive: true });
const staging = path.join(dest, ".staging");
rmSync(staging, { recursive: true, force: true });
mkdirSync(staging);
const zip = path.join(staging, "aria2.zip");
writeFileSync(zip, bytes);
// Windows' bundled bsdtar extracts zip archives.
execFileSync(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe"), [
  "-xf",
  zip,
  "-C",
  staging,
]);
for (const file of ["aria2c.exe", "COPYING", "LICENSE.OpenSSL", "AUTHORS"]) {
  copyFileSync(path.join(staging, PIN.folder, file), path.join(dest, file));
}
rmSync(staging, { recursive: true, force: true });
console.log(
  `fetch-aria2: installed aria2 ${PIN.version} (sha256 verified) to ${path.relative(root, dest)}`,
);
