import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { installedMatches, platformPin } from "../fetch-ytdlp.mjs";

test("all three supported package targets have an immutable yt-dlp asset pin", () => {
  for (const [os, arch] of [
    ["win32", "x64"],
    ["darwin", "arm64"],
    ["linux", "x64"],
  ]) {
    const pin = platformPin(os, arch);
    assert.match(pin.sha256, /^[a-f0-9]{64}$/);
    assert.ok(pin.size > 1_000_000);
    assert.equal(pin.file, os === "win32" ? "yt-dlp.exe" : "yt-dlp");
  }
  assert.throws(() => platformPin("win32", "ia32"), /no yt-dlp pin/);
  assert.deepEqual(platformPin("darwin", "x64"), platformPin("darwin", "arm64"));
});

test("a missing or corrupt sidecar is never trusted from a cache marker", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-ytdlp-"));
  const data = Buffer.from("verified fixture binary");
  const pin = {
    file: "yt-dlp",
    size: data.length,
    sha256: createHash("sha256").update(data).digest("hex"),
  };
  try {
    assert.equal(installedMatches(dir, pin), false);
    fs.writeFileSync(path.join(dir, pin.file), data);
    assert.equal(installedMatches(dir, pin), true);
    fs.writeFileSync(path.join(dir, pin.file), Buffer.alloc(data.length, 0));
    assert.equal(installedMatches(dir, pin), false);
    fs.writeFileSync(path.join(dir, pin.file), data.subarray(1));
    assert.equal(installedMatches(dir, pin), false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
