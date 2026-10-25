#!/usr/bin/env node
// B0.5 native packaging (docs/reports/b0.5-packaging-smoke.md).
//
//   node scripts/package.mjs [--smoke]
//
// 1. Builds release `dm-service` and `dm` for the host target triple.
// 2. Stages them as Tauri `externalBin` sidecars: apps/desktop/src-tauri/binaries/<name>-<triple>[.exe]
//    (Tauri strips the triple and installs them next to the main executable).
// 3. Runs `tauri build` with tauri.package.conf.json merged in, for the bundles of this OS:
//    Windows nsis+msi, macOS app+dmg (ad-hoc signed), Linux deb+appimage.
//    `--smoke` enables the desktop's test-only `smoke` feature; such packages are internal test
//    artifacts and are labelled so.
// 4. Copies the bundles to dist-packages/<os>-<arch>[-smoke-test]/ with SHA256SUMS.txt and
//    build-info.json.
//
// The aria2 sidecar is never bundled (ADR 0005, REL-05).
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const smoke = process.argv.includes("--smoke");
const isWin = process.platform === "win32";
const exe = isWin ? ".exe" : "";

function run(cmd, args, opts = {}) {
  console.log(`$ ${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, { stdio: "inherit", cwd: root, shell: isWin, ...opts });
  if (r.status !== 0) {
    console.error(`package: ${cmd} failed with ${r.status}`);
    process.exit(r.status ?? 1);
  }
}

const rustcInfo = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
const triple = /^host: (.+)$/m.exec(rustcInfo)?.[1]?.trim();
if (!triple) throw new Error("cannot determine the host target triple");
const arch = triple.split("-")[0];
const os = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform];
const bundles = { windows: "nsis,msi", macos: "app,dmg", linux: "deb,appimage" }[os];
if (!bundles) throw new Error(`unsupported platform ${process.platform}`);

// 1. Release sidecars.
run("cargo", ["build", "--release", "--locked", "-p", "dm-service", "-p", "dm-cli"]);

// 2. Stage them with target-triple names.
const binDir = path.join(root, "apps/desktop/src-tauri/binaries");
fs.mkdirSync(binDir, { recursive: true });
for (const name of ["dm-service", "dm"]) {
  const from = path.join(root, "target/release", `${name}${exe}`);
  const to = path.join(binDir, `${name}-${triple}${exe}`);
  fs.copyFileSync(from, to);
  console.log(`staged ${path.relative(root, to)}`);
}

// 3. Tauri bundles.
const tauriArgs = [
  "--filter",
  "@dm/desktop",
  "tauri",
  "build",
  "--bundles",
  bundles,
  "--config",
  "src-tauri/tauri.package.conf.json",
  "--ci",
];
if (smoke) tauriArgs.push("--features", "smoke");
const pnpm = process.env.npm_execpath?.includes("pnpm") ? "pnpm" : "npx";
run(pnpm, pnpm === "pnpm" ? tauriArgs : ["--yes", "pnpm@12.14.0", ...tauriArgs]);

// 4. Collect.
const label = `${os}-${arch}${smoke ? "-smoke-test" : ""}`;
const outDir = path.join(root, "dist-packages", label);
fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
const bundleRoot = path.join(root, "target/release/bundle");
const wanted = /\.(exe|msi|dmg|deb|AppImage)$/;
const files = [];
function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory() && !entry.name.endsWith(".app")) walk(p);
    else if (entry.isFile() && wanted.test(entry.name)) files.push(p);
  }
}
walk(bundleRoot);
const sums = [];
for (const f of files) {
  const dest = path.join(outDir, path.basename(f));
  fs.copyFileSync(f, dest);
  const sha = createHash("sha256").update(fs.readFileSync(dest)).digest("hex");
  sums.push(`${sha}  ${path.basename(f)}`);
}
// The macOS .app is a directory: archive it so it can be uploaded and checksummed.
if (os === "macos") {
  const app = fs.readdirSync(path.join(bundleRoot, "macos")).find((n) => n.endsWith(".app"));
  if (app) {
    const zip = path.join(outDir, `${app}.zip`);
    run("ditto", ["-c", "-k", "--keepParent", path.join(bundleRoot, "macos", app), zip]);
    const sha = createHash("sha256").update(fs.readFileSync(zip)).digest("hex");
    sums.push(`${sha}  ${path.basename(zip)}`);
  }
}
fs.writeFileSync(path.join(outDir, "SHA256SUMS.txt"), sums.join("\n") + "\n");
const info = {
  label,
  target_triple: triple,
  os,
  arch,
  bundles: bundles.split(","),
  smoke_feature: smoke,
  signing: os === "macos" ? "ad-hoc (codesign -), not notarised" : "unsigned",
  aria2_bundled: false,
  git_commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
  built_at: new Date().toISOString(),
  files: sums,
};
fs.writeFileSync(path.join(outDir, "build-info.json"), JSON.stringify(info, null, 2) + "\n");
console.log(`\npackages in ${path.relative(root, outDir)}:\n${sums.join("\n")}`);
if (files.length === 0) {
  console.error("package: no bundles were produced");
  process.exit(1);
}
