#!/usr/bin/env node
// B0.5 native packaging (docs/reports/b0.5-packaging-smoke.md).
//
//   node scripts/package.mjs [--smoke] [--updater-config FILE [--update-fixture]]
//
// 1. Builds release `dm-service`, `dm` and `dm-browser-host` for the host target triple.
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
//
// Updates (ADV09, ADR 0030): a normal package has no updater configuration and is refused while
// updater signing variables are set. `--updater-config FILE` merges a trusted updater config
// (HTTPS endpoints, public key, requireSignedVersion, createUpdaterArtifacts) and needs the signing
// key in TAURI_SIGNING_PRIVATE_KEY(_PATH); `--update-fixture` additionally allows a loopback HTTP
// test endpoint and compiles the shell's `update-fixture` feature. See scripts/lib/updater-config.mjs.
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { checkUpdaterSetup } from "./lib/updater-config.mjs";
import { writeZip } from "./lib/zip.mjs";

const root = path.resolve(import.meta.dirname, "..");
const smoke = process.argv.includes("--smoke");
const argAfter = (flag) => {
  const i = process.argv.indexOf(flag);
  return i >= 0 ? process.argv[i + 1] : undefined;
};
const updaterConfigPath = argAfter("--updater-config");
const updateFixture = process.argv.includes("--update-fixture");
const updaterConfig = updaterConfigPath
  ? JSON.parse(fs.readFileSync(path.resolve(updaterConfigPath), "utf8"))
  : null;
{
  const problems = checkUpdaterSetup({
    config: updaterConfig,
    fixture: updateFixture,
    env: process.env,
  });
  if (problems.length > 0) {
    for (const problem of problems) console.error(`package: updater: ${problem}`);
    process.exit(2);
  }
}
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
run("cargo", [
  "build",
  "--release",
  "--locked",
  "-p",
  "dm-service",
  "-p",
  "dm-cli",
  "-p",
  "dm-browser-host",
]);

// 2. Stage them with target-triple names.
const binDir = path.join(root, "apps/desktop/src-tauri/binaries");
fs.mkdirSync(binDir, { recursive: true });
for (const name of ["dm-service", "dm", "dm-browser-host"]) {
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
const features = [...(smoke ? ["smoke"] : []), ...(updateFixture ? ["update-fixture"] : [])];
if (features.length > 0) tauriArgs.push("--features", features.join(","));
if (updaterConfigPath) tauriArgs.push("--config", path.resolve(updaterConfigPath));
const pnpm = process.env.npm_execpath?.includes("pnpm") ? "pnpm" : "npx";
// `tauri build` reads the updater key only from TAURI_SIGNING_PRIVATE_KEY (its content); a key given
// by path is read here and passed to that one child process only.
const buildEnv = { ...process.env };
if (updaterConfig && !buildEnv.TAURI_SIGNING_PRIVATE_KEY && buildEnv.TAURI_SIGNING_PRIVATE_KEY_PATH)
  buildEnv.TAURI_SIGNING_PRIVATE_KEY = fs
    .readFileSync(path.resolve(buildEnv.TAURI_SIGNING_PRIVATE_KEY_PATH), "utf8")
    .trim();
run(pnpm, pnpm === "pnpm" ? tauriArgs : ["--yes", "pnpm@12.14.0", ...tauriArgs], {
  env: buildEnv,
});

// 4. Collect.
const label = `${os}-${arch}${smoke ? "-smoke-test" : ""}${updateFixture ? "-update-fixture" : ""}`;
const outDir = path.join(root, "dist-packages", label);
fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
const bundleRoot = path.join(root, "target/release/bundle");
// Updater artifacts (`.sig` signatures, macOS/Linux `.tar.gz`) only exist with an updater config.
const wanted = updaterConfig
  ? /\.(exe|msi|dmg|deb|AppImage|sig|tar\.gz)$/
  : /\.(exe|msi|dmg|deb|AppImage)$/;
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
// The browser extension, built in its NORMAL mode (never `--mode e2e`): capture permissions stay
// optional and no test hook is compiled in. It is shipped unpacked (load it with "Load unpacked")
// with a hash per file and one over the sorted list.
const extOut = path.join(outDir, "extension-unpacked");
const extArgs = [
  "--filter",
  "@dm/extension",
  "exec",
  "vite",
  "build",
  "--outDir",
  extOut,
  "--emptyOutDir",
];
run(pnpm, pnpm === "pnpm" ? extArgs : ["--yes", "pnpm@12.14.0", ...extArgs]);
const extManifest = JSON.parse(fs.readFileSync(path.join(extOut, "manifest.json"), "utf8"));
const grantedUpFront = [
  ...(extManifest.permissions ?? []),
  ...(extManifest.host_permissions ?? []),
];
if (grantedUpFront.some((p) => ["downloads", "webRequest"].includes(p) || p.includes("://"))) {
  console.error("package: the normal extension build grants capture permissions up front");
  process.exit(1);
}
const extFiles = [];
(function walkExt(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walkExt(p);
    else extFiles.push(p);
  }
})(extOut);
const extSums = extFiles
  .map((f) => [
    createHash("sha256").update(fs.readFileSync(f)).digest("hex"),
    path.relative(extOut, f).replaceAll("\\", "/"),
  ])
  .sort((a, b) => a[1].localeCompare(b[1]))
  .map(([sha, rel]) => `${sha}  ${rel}`);
fs.writeFileSync(path.join(outDir, "EXTENSION-SHA256SUMS.txt"), extSums.join("\n") + "\n");
const extensionSha = createHash("sha256").update(extSums.join("\n")).digest("hex");
for (const leftover of ["popup.html", "options.html", "background.js"])
  if (!fs.existsSync(path.join(extOut, leftover))) {
    console.error(`package: the extension build lacks ${leftover}`);
    process.exit(1);
  }
if (
  fs
    .readdirSync(extOut, { recursive: true })
    .some(
      (n) =>
        String(n).endsWith(".js") &&
        fs.readFileSync(path.join(extOut, String(n)), "utf8").includes("downovaE2E"),
    )
) {
  console.error("package: the normal extension build contains the e2e hook");
  process.exit(1);
}
// B3.4 (ADR 0029): the Firefox build, also in its NORMAL mode, from the same sources: explicit
// public hand-offs only (no capture, cookie or sign-in permission), the development add-on ID the
// host allows. Shipped unpacked and as an UNSIGNED .xpi: Firefox loads it only temporarily
// (about:debugging → Load Temporary Add-on); a permanent install needs Mozilla's signature.
const ffOut = path.join(outDir, "extension-firefox-unpacked");
const ffArgs = [
  "--filter",
  "@dm/extension",
  "exec",
  "vite",
  "build",
  "--mode",
  "firefox",
  "--outDir",
  ffOut,
  "--emptyOutDir",
];
run(pnpm, pnpm === "pnpm" ? ffArgs : ["--yes", "pnpm@12.14.0", ...ffArgs]);
const ffManifest = JSON.parse(fs.readFileSync(path.join(ffOut, "manifest.json"), "utf8"));
const ffText = JSON.stringify(ffManifest);
if (
  !ffManifest.browser_specific_settings?.gecko?.id ||
  !ffManifest.background?.scripts ||
  ffManifest.background?.service_worker ||
  ["cookies", "downloads", "webRequest", "optional_permissions", "host_permissions"].some((k) =>
    ffText.includes(`"${k}"`),
  )
) {
  console.error("package: the Firefox build's manifest is not the normal Firefox manifest");
  process.exit(1);
}
const ffFiles = fs
  .readdirSync(ffOut, { recursive: true })
  .map((n) => String(n).replaceAll("\\", "/"))
  .filter((n) => fs.statSync(path.join(ffOut, n)).isFile())
  .sort();
if (
  ffFiles.some(
    (n) => n.endsWith(".js") && fs.readFileSync(path.join(ffOut, n), "utf8").includes("downovaE2E"),
  )
) {
  console.error("package: the normal Firefox build contains the e2e hook");
  process.exit(1);
}
if (ffFiles.some((n) => n.startsWith("reconnect"))) {
  console.error("package: the Firefox build contains the Chromium-only reconnect page");
  process.exit(1);
}
const ffSums = ffFiles.map(
  (n) =>
    `${createHash("sha256")
      .update(fs.readFileSync(path.join(ffOut, n)))
      .digest("hex")}  ${n}`,
);
fs.writeFileSync(path.join(outDir, "EXTENSION-FIREFOX-SHA256SUMS.txt"), ffSums.join("\n") + "\n");
const xpi = path.join(outDir, "downova-firefox-unsigned.xpi");
writeZip(
  xpi,
  ffFiles.map((n) => ({ name: n, data: fs.readFileSync(path.join(ffOut, n)) })),
);
sums.push(
  `${createHash("sha256").update(fs.readFileSync(xpi)).digest("hex")}  ${path.basename(xpi)}`,
);
fs.writeFileSync(path.join(outDir, "SHA256SUMS.txt"), sums.join("\n") + "\n");
// Exact source identity: the commit plus whatever was uncommitted when this was built.
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" });
const porcelain = git("status", "--porcelain").split("\n").filter(Boolean);
const diff = execFileSync("git", ["diff", "HEAD", "--binary"], { cwd: root });
const info = {
  purpose: smoke
    ? "internal smoke-test artifact (test-only smoke feature compiled in); never for users"
    : "internal unsigned test build; not a release",
  source: {
    commit: git("rev-parse", "HEAD").trim(),
    uncommitted_paths: porcelain,
    uncommitted_diff_sha256: porcelain.length
      ? createHash("sha256").update(diff).digest("hex")
      : null,
  },
  extension: {
    mode: "production (optional capture permissions, no e2e hooks)",
    version: extManifest.version,
    files_sha256: extensionSha,
  },
  extension_firefox: {
    mode: "firefox (public hand-off only, no e2e hooks); unsigned: temporary loading only",
    addon_id: ffManifest.browser_specific_settings.gecko.id,
    version: ffManifest.version,
    files_sha256: createHash("sha256").update(ffSums.join("\n")).digest("hex"),
    xpi: path.basename(xpi),
  },
  label,
  target_triple: triple,
  os,
  arch,
  bundles: bundles.split(","),
  smoke_feature: smoke,
  // ADV09: "none" (no in-app update), "configured" (trusted updater config) or "fixture".
  updater: updaterConfig ? (updateFixture ? "fixture" : "configured") : "none",
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
