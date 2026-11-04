// node --test scripts/acceptance/build-kit.test.mjs (part of `pnpm verify`, group structure).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const script = path.join(import.meta.dirname, "build-kit.mjs");
const sha = (b) => createHash("sha256").update(b).digest("hex");

/** A fake normal package: two installers, an extension, sums and build-info. */
function fakePackage({ smoke = false, key = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-kitpkg-"));
  const files = {
    "Downova_0.1.0_x64-setup.exe": "setup-bytes",
    "Downova_0.1.0_x64_en-US.msi": "msi-bytes",
  };
  for (const [n, b] of Object.entries(files)) fs.writeFileSync(path.join(dir, n), b);
  fs.writeFileSync(
    path.join(dir, "SHA256SUMS.txt"),
    Object.entries(files)
      .map(([n, b]) => `${sha(b)}  ${n}`)
      .join("\n") + "\n",
  );
  const ext = path.join(dir, "extension-unpacked");
  fs.mkdirSync(ext);
  const manifest = JSON.stringify({ name: "Downova", ...(key ? { key: "AAAA" } : {}) });
  fs.writeFileSync(path.join(ext, "manifest.json"), manifest);
  fs.writeFileSync(path.join(ext, "background.js"), "// bg");
  fs.writeFileSync(
    path.join(dir, "EXTENSION-SHA256SUMS.txt"),
    `${sha(manifest)}  manifest.json\n${sha("// bg")}  background.js\n`,
  );
  fs.writeFileSync(
    path.join(dir, "build-info.json"),
    JSON.stringify({
      label: smoke ? "windows-x86_64-smoke-test" : "windows-x86_64",
      os: "windows",
      arch: "x86_64",
      signing: "unsigned",
      smoke_feature: smoke,
      source: { commit: "0".repeat(40), uncommitted_paths: [] },
    }),
  );
  return dir;
}
const build = (pkg, out) =>
  spawnSync(process.execPath, [script, "--package-dir", pkg, "--out", out, "--skip-source-check"], {
    encoding: "utf8",
  });

test("a normal package becomes a verifiable kit with one result row per checklist test", () => {
  const pkg = fakePackage();
  const out = path.join(pkg, "kit");
  const r = build(pkg, out);
  assert.equal(r.status, 0, r.stderr);
  for (const f of [
    "package/Downova_0.1.0_x64-setup.exe",
    "package/build-info.json",
    "extension/manifest.json",
    "fixtures/acceptance/fixture-server.mjs",
    "fixtures/perf/fixture-process.mjs",
    "fixtures/lib/fixture.mjs",
    "CHECKLIST.md",
    "QUICKSTART-bn.md",
    "RESULTS.csv",
    "KIT-INFO.json",
    "verify-kit.mjs",
    "SHA256SUMS.txt",
    "evidence/README.txt",
  ])
    assert.ok(fs.existsSync(path.join(out, f)), f);
  const checklist = fs.readFileSync(path.join(out, "CHECKLIST.md"), "utf8");
  // No links into a repository the tester does not have.
  assert.doesNotMatch(checklist, /\]\(\.\.\//);
  assert.match(checklist, /\(repository: docs\/reports\/b2\.32-installed-acceptance-runbook\.md\)/);
  const ids = [...checklist.matchAll(/^\| ([A-Z]\d+) \|/gm)].map((m) => m[1]);
  const rows = fs.readFileSync(path.join(out, "RESULTS.csv"), "utf8").trim().split("\n").slice(1);
  assert.deepEqual(
    rows.map((r) => r.split(",")[0]),
    ids,
  );
  assert.ok(rows.every((r) => r.includes(sha("setup-bytes")) && r.includes("NOT RUN")));
  const info = JSON.parse(fs.readFileSync(path.join(out, "KIT-INFO.json"), "utf8"));
  assert.equal(info.package.smoke_feature, false);
  assert.deepEqual(info.missing_platforms, ["macos-aarch64", "linux-x86_64"]);
  // The kit verifies itself, and catches a changed file.
  const ok = spawnSync(process.execPath, ["verify-kit.mjs"], { cwd: out, encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stdout);
  fs.appendFileSync(path.join(out, "package/Downova_0.1.0_x64-setup.exe"), "x");
  const bad = spawnSync(process.execPath, ["verify-kit.mjs"], { cwd: out, encoding: "utf8" });
  assert.equal(bad.status, 1);
  assert.match(bad.stdout, /MISMATCH package\/Downova_0\.1\.0_x64-setup\.exe/);
  fs.rmSync(pkg, { recursive: true, force: true });
});

test("a smoke-feature package is refused", () => {
  const pkg = fakePackage({ smoke: true });
  const r = build(pkg, path.join(pkg, "kit"));
  assert.equal(r.status, 1);
  assert.match(r.stderr, /smoke-feature test package/);
  fs.rmSync(pkg, { recursive: true, force: true });
});

test("a tampered installer or extension file is refused", () => {
  const pkg = fakePackage();
  fs.appendFileSync(path.join(pkg, "Downova_0.1.0_x64_en-US.msi"), "!");
  let r = build(pkg, path.join(pkg, "kit"));
  assert.equal(r.status, 1);
  assert.match(r.stderr, /does not match SHA256SUMS/);
  fs.rmSync(pkg, { recursive: true, force: true });

  const pkg2 = fakePackage();
  fs.appendFileSync(path.join(pkg2, "extension-unpacked", "background.js"), "!");
  r = build(pkg2, path.join(pkg2, "kit"));
  assert.equal(r.status, 1);
  assert.match(r.stderr, /extension file background\.js/);
  fs.rmSync(pkg2, { recursive: true, force: true });
});

test("an extension without a manifest key is refused (its ID would not match the helper)", () => {
  const pkg = fakePackage({ key: false });
  const r = build(pkg, path.join(pkg, "kit"));
  assert.equal(r.status, 1);
  assert.match(r.stderr, /manifest key/);
  fs.rmSync(pkg, { recursive: true, force: true });
});
