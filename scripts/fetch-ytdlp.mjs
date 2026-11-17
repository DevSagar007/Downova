#!/usr/bin/env node
// Optional standalone sidecar, never bundled by package.mjs. Immutable official release assets;
// sizes and SHA-256 digests from github.com/yt-dlp/yt-dlp/releases/tag/2026.10.26 (GitHub API).
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { downloadVerified } from "./lib/download.mjs";

export const VERSION = "2026.10.26";
const pins = {
  "win32-x64": {
    asset: "yt-dlp.exe",
    size: 23575744,
    sha256: "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d",
  },
  "darwin-arm64": {
    asset: "yt-dlp_macos",
    size: 37466304,
    sha256: "0f922c15ccc0b83eefa1c8bc9104e434318c4bf16e408e1ee93b7745a6efe0aa",
  },
  // The same upstream universal executable runs on Intel verification runners too. This does
  // not add an Intel macOS desktop package target.
  "darwin-x64": {
    asset: "yt-dlp_macos",
    size: 37466304,
    sha256: "0f922c15ccc0b83eefa1c8bc9104e434318c4bf16e408e1ee93b7745a6efe0aa",
  },
  "linux-x64": {
    asset: "yt-dlp_linux",
    size: 41166968,
    sha256: "767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795",
  },
};
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dest = path.join(root, "tools", "ytdlp");

export function platformPin(platform = process.platform, arch = process.arch) {
  const pin = pins[`${platform}-${arch}`];
  if (!pin)
    throw new Error(
      `no yt-dlp pin for ${platform}-${arch}; set DM_YTDLP_PATH to a user-installed binary`,
    );
  return { ...pin, file: platform === "win32" ? "yt-dlp.exe" : "yt-dlp" };
}

export function installedMatches(dir, pin = platformPin()) {
  try {
    const file = path.join(dir, pin.file);
    return (
      fs.statSync(file).size === pin.size &&
      createHash("sha256").update(fs.readFileSync(file)).digest("hex") === pin.sha256
    );
  } catch {
    return false;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const pin = platformPin();
    if (!process.argv.includes("--force") && installedMatches(dest, pin)) {
      console.log(`fetch-ytdlp: verified ${VERSION} already installed`);
    } else {
      const url = `https://github.com/yt-dlp/yt-dlp/releases/download/${VERSION}/${pin.asset}`;
      const bytes = await downloadVerified(url, {
        tool: "yt-dlp",
        size: pin.size,
        sha256: pin.sha256,
      });
      fs.mkdirSync(dest, { recursive: true });
      const staging = path.join(dest, `.${pin.file}.${process.pid}.tmp`);
      try {
        fs.writeFileSync(staging, bytes, { flag: "wx", mode: 0o755 });
        fs.renameSync(staging, path.join(dest, pin.file));
      } finally {
        fs.rmSync(staging, { force: true });
      }
      console.log(`fetch-ytdlp: installed verified ${VERSION} in tools/ytdlp/`);
    }
  } catch (e) {
    console.error(`fetch-ytdlp: ${e.message}`);
    process.exitCode = 1;
  }
}
