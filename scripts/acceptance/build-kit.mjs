#!/usr/bin/env node
// Builds the B2 installed-app acceptance kit from a NORMAL package (B2.34).
//
//   node scripts/acceptance/build-kit.mjs [--package-dir dist-packages/<os>-<arch>]
//        [--out dist-packages/acceptance-kit-b2-<label>] [--zip] [--skip-source-check]
//        [--extension-id <id>]
//
// What it does, and refuses:
// - Reads the package's build-info.json; refuses a smoke-feature (test) package.
// - Verifies the installers against the package's SHA256SUMS.txt and every extension file against
//   EXTENSION-SHA256SUMS.txt; any mismatch or missing file stops it. The extension ID derived from
//   its manifest key must equal the ID the native helper allows (`DEV_EXTENSION_ID` in
//   crates/browser-host/src/lib.rs, or `--extension-id`).
// - Compares the package's source commit with HEAD over the production paths. A change outside test
//   files stops it (rebuild the package first); test-only changes are listed in KIT-INFO.json.
//   `--skip-source-check` is for the unit test only (no git repository there).
// - Copies the installers, the production extension, the fixture scripts with their one relative
//   import (`lib/fixture.mjs`, which needs only Node built-ins), the checklist, the Bangla quick-start.
// - Generates RESULTS.csv (one row per test id in the checklist), KIT-INFO.json, verify-kit.mjs, an
//   empty evidence/ folder, and SHA256SUMS.txt over every kit file. Large fixture data is generated at
//   run time by the fixture scripts, never stored in the kit.
// - Only known kit paths are allowed (package/, extension/, fixtures/, evidence/ and the top-level
//   files); anything else stops it, so no cache, node_modules or payload can slip in.
// - `--zip` writes <out>.zip with scripts/lib/zip.mjs (the same on every OS, B2.35; GNU tar cannot
//   write ZIP), plus <out>.zip.sha256 and <out>.zip.json (bytes, SHA-256, label, source commit).
//   The AppImage and verify-kit.mjs keep their executable bit.
// - Per platform the "primary" installer (prefilled in RESULTS.csv) is the NSIS setup on Windows,
//   the .dmg on macOS and the .deb on Linux; QUICKSTART-bn.md starts with a generated block naming
//   this kit's platform, files and hashes.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { entriesFromDir, writeZip } from "../lib/zip.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(
  arg("--package-dir", path.join(root, "dist-packages/windows-x86_64")),
);
const skipSource = argv.includes("--skip-source-check");
const fail = (msg) => {
  console.error(`build-kit: ${msg}`);
  process.exit(1);
};
const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const readSums = (file) =>
  Object.fromEntries(
    fs
      .readFileSync(file, "utf8")
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const m = /^([0-9a-f]{64})\s+\*?(.+)$/.exec(l);
        if (!m) fail(`malformed line in ${file}: ${l}`);
        return [m[2], m[1]];
      }),
  );

// ------------------------------------------------------------------ package identity
const infoFile = path.join(packageDir, "build-info.json");
if (!fs.existsSync(infoFile)) fail(`no build-info.json in ${packageDir}`);
const info = JSON.parse(fs.readFileSync(infoFile, "utf8"));
if (info.smoke_feature !== false || /smoke/.test(info.label ?? ""))
  fail(`${info.label}: a smoke-feature test package is never an acceptance candidate`);
// B3.6: an update-fixture build trusts a loopback endpoint and a disposable key: never a candidate.
if ((info.updater ?? "none") !== "none" || (info.build_kind ?? "normal") !== "normal")
  fail(`${info.label}: only a normal package (no updater or fixture configuration) is a candidate`);
const sums = readSums(path.join(packageDir, "SHA256SUMS.txt"));
const installers = Object.keys(sums);
if (!installers.length) fail("SHA256SUMS.txt lists no installer");
for (const name of installers) {
  const file = path.join(packageDir, name);
  if (!fs.existsSync(file)) fail(`missing ${name}`);
  if (sha256(file) !== sums[name]) fail(`${name} does not match SHA256SUMS.txt`);
}
// The installer a tester starts with on this OS (prefilled in RESULTS.csv).
const primaryPattern = { windows: /-setup\.exe$/, macos: /\.dmg$/, linux: /\.deb$/ }[info.os];
const setup = installers.find((n) => primaryPattern?.test(n));
if (!setup) fail(`no primary installer for ${info.os} among ${installers.join(", ")}`);
const extDir = path.join(packageDir, "extension-unpacked");
const extSums = readSums(path.join(packageDir, "EXTENSION-SHA256SUMS.txt"));
for (const [rel, want] of Object.entries(extSums)) {
  const file = path.join(extDir, rel);
  if (!fs.existsSync(file)) fail(`extension file missing: ${rel}`);
  if (sha256(file) !== want) fail(`extension file ${rel} does not match EXTENSION-SHA256SUMS.txt`);
}
const extManifest = JSON.parse(fs.readFileSync(path.join(extDir, "manifest.json"), "utf8"));
if (!extManifest.key) fail("the extension has no manifest key (its ID would not match the helper)");
// Chrome's extension ID: the first 128 bits of SHA-256 over the DER public key, as a–p letters.
const extensionId = [
  ...createHash("sha256").update(Buffer.from(extManifest.key, "base64")).digest("hex").slice(0, 32),
]
  .map((c) => String.fromCharCode(97 + parseInt(c, 16)))
  .join("");
const allowedId =
  arg("--extension-id") ??
  /DEV_EXTENSION_ID: &str = "([a-p]{32})"/.exec(
    fs.readFileSync(path.join(root, "crates/browser-host/src/lib.rs"), "utf8"),
  )?.[1];
if (!allowedId) fail("cannot read the helper's allowed extension ID");
if (extensionId !== allowedId)
  fail(`the extension's ID ${extensionId} is not the ID the native helper allows (${allowedId})`);

// ------------------------------------------------------------------ source identity
const buildCommit = info.source?.commit ?? info.git_commit ?? null;
let source = { build_commit: buildCommit, checked: false };
if (!skipSource) {
  if (info.source?.uncommitted_paths?.length)
    fail(
      `the package was built with uncommitted changes: ${info.source.uncommitted_paths.join(", ")}`,
    );
  const git = (...a) => spawnSync("git", a, { cwd: root, encoding: "utf8" });
  const head = git("rev-parse", "HEAD").stdout.trim();
  const productionPaths = [
    "crates",
    "apps/desktop/src",
    "apps/desktop/src-tauri",
    "apps/extension/src",
    "apps/extension/public",
    "packages",
  ];
  const d = git("diff", "--name-only", buildCommit, head, "--", ...productionPaths);
  if (d.status !== 0) fail(`git diff ${buildCommit}..${head} failed: ${d.stderr}`);
  const changed = d.stdout.split("\n").filter(Boolean);
  const isTest = (p) => /(\.test\.[cm]?[jt]sx?|_test\.rs)$|\/tests\//.test(p);
  const production = changed.filter((p) => !isTest(p));
  if (production.length)
    fail(
      `production code changed since the package's commit ${buildCommit}: ${production.join(", ")}. Rebuild the package.`,
    );
  source = {
    build_commit: buildCommit,
    head_commit: head,
    checked: true,
    production_paths: productionPaths,
    production_changes_since_build: [],
    test_only_changes_since_build: changed,
  };
}

// ------------------------------------------------------------------ assemble
const label = info.label;
const out = path.resolve(
  arg("--out", path.join(root, "dist-packages", `acceptance-kit-b2-${label}`)),
);
fs.rmSync(out, { recursive: true, force: true });
const copy = (from, to) => {
  fs.mkdirSync(path.dirname(path.join(out, to)), { recursive: true });
  fs.cpSync(from, path.join(out, to), { recursive: true });
};
for (const name of installers) copy(path.join(packageDir, name), path.join("package", name));
copy(infoFile, "package/build-info.json");
copy(extDir, "extension");
// The fixtures keep their relative layout, so `../lib/fixture.mjs` still resolves.
copy(
  path.join(root, "scripts/acceptance/fixture-server.mjs"),
  "fixtures/acceptance/fixture-server.mjs",
);
copy(path.join(root, "scripts/perf/fixture-process.mjs"), "fixtures/perf/fixture-process.mjs");
copy(path.join(root, "scripts/lib/fixture.mjs"), "fixtures/lib/fixture.mjs");
// The kit has no repository: links to repository documents become plain text naming them.
const delink = (md) =>
  md.replace(
    /\[([^\]]+)\]\((\.\.\/[^)\s]+)\)/g,
    (_, text, href) =>
      `${text} (repository: docs/${href.replace(/^(\.\.\/)+/, "").replace(/#.*$/, "")})`,
  );
fs.writeFileSync(
  path.join(out, "CHECKLIST.md"),
  delink(fs.readFileSync(path.join(root, "docs/acceptance/b2-checklist.md"), "utf8")),
);
const platformName = { windows: "Windows", macos: "macOS", linux: "Linux" }[info.os] ?? info.os;
const hashCmd = {
  windows: (n) => `Get-FileHash .\\package\\${n} -Algorithm SHA256`,
  macos: (n) => `shasum -a 256 package/${n}`,
  linux: (n) => `sha256sum package/${n}`,
}[info.os];
const header = [
  `> **এই kit-এর package (স্বয়ংক্রিয়ভাবে তৈরি):** ${platformName} ${info.arch}, ${info.signing}, smoke_feature: ${info.smoke_feature}, source \`${buildCommit}\`.`,
  ">",
  ...installers.map(
    (n) => `> - \`package/${n}\`: SHA-256 \`${sums[n]}\`${n === setup ? " (মূল installer)" : ""}`,
  ),
  ">",
  `> Hash মেলান: \`${hashCmd(setup)}\``,
  "",
  "",
].join("\n");
fs.writeFileSync(
  path.join(out, "QUICKSTART-bn.md"),
  header + fs.readFileSync(path.join(root, "docs/acceptance/b2-quickstart-bn.md"), "utf8"),
);
fs.mkdirSync(path.join(out, "evidence"), { recursive: true });
fs.writeFileSync(
  path.join(out, "evidence", "README.txt"),
  "Put screenshots, logs and fixture-hashes.json here; name each file in RESULTS.csv.\n",
);

// RESULTS.csv: one row per test id, in checklist order.
const checklist = fs.readFileSync(path.join(root, "docs/acceptance/b2-checklist.md"), "utf8");
// Row IDs are one to three capital letters and a number (C1, FX1, UP1, IL1). Until B3.6 only one
// letter was matched, so the FX and UP rows were missing from RESULTS.csv.
const ids = [...checklist.matchAll(/^\| ([A-Z]{1,3}\d+) \|/gm)].map((m) => m[1]);
if (new Set(ids).size !== ids.length) fail("the checklist repeats a test id");
const columns = [
  "test_id",
  "platform",
  "os_version",
  "browser",
  "browser_version",
  "package_sha256",
  "observer",
  "result",
  "evidence",
  "notes",
];
fs.writeFileSync(
  path.join(out, "RESULTS.csv"),
  [
    columns.join(","),
    ...ids.map((id) => [id, "", "", "", "", sums[setup], "", "NOT RUN", "", ""].join(",")),
  ].join("\n") + "\n",
);

const allPlatforms = ["windows-x86_64", "macos-aarch64", "linux-x86_64"];
const kitInfo = {
  kit: "Downova B2 installed-app acceptance kit",
  created_at: new Date().toISOString(),
  package: {
    label,
    os: info.os,
    arch: info.arch,
    signing: info.signing,
    smoke_feature: info.smoke_feature,
    build_kind: info.build_kind ?? "normal",
    version: info.version ?? null,
    components: info.components ?? null,
    notices: info.notices
      ? {
          file: info.notices.file,
          distributed_components: info.notices.distributed_components,
          with_unresolved_metadata: info.notices.with_unresolved_metadata,
        }
      : null,
    files: Object.fromEntries(
      installers.map((n) => [
        n,
        { sha256: sums[n], bytes: fs.statSync(path.join(packageDir, n)).size },
      ]),
    ),
    primary_installer: setup,
    setup_sha256: sums[setup],
    extension_id: extensionId,
    extension_files_sha256: info.extension?.files_sha256 ?? null,
    built_at: info.built_at,
  },
  source,
  missing_platforms: allPlatforms.filter((p) => p !== label),
  test_ids: ids,
};
fs.writeFileSync(path.join(out, "KIT-INFO.json"), JSON.stringify(kitInfo, null, 2) + "\n");
fs.writeFileSync(path.join(out, "verify-kit.mjs"), verifyScript());

// SHA256SUMS.txt over every file in the kit (itself excluded).
const files = [];
const walk = (d) => {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else files.push(path.relative(out, p).split(path.sep).join("/"));
  }
};
walk(out);
files.sort();
const allowedTop = new Set([
  "CHECKLIST.md",
  "QUICKSTART-bn.md",
  "RESULTS.csv",
  "KIT-INFO.json",
  "verify-kit.mjs",
]);
const allowedDirs = ["package/", "extension/", "fixtures/", "evidence/"];
const stray = files.filter(
  (f) =>
    (!allowedTop.has(f) && !allowedDirs.some((d) => f.startsWith(d))) ||
    /(^|\/)(node_modules|\.git|target)\//.test(f),
);
if (stray.length) fail(`unexpected files in the kit: ${stray.join(", ")}`);
fs.writeFileSync(
  path.join(out, "SHA256SUMS.txt"),
  files.map((f) => `${sha256(path.join(out, f))}  ${f}`).join("\n") + "\n",
);

let zip = null;
let zipInfo = null;
if (argv.includes("--zip")) {
  zip = `${out}.zip`;
  const executable = (rel) => rel === "verify-kit.mjs" || /\.AppImage$/.test(rel);
  writeZip(zip, entriesFromDir(out, path.basename(out), executable));
  zipInfo = {
    zip: path.basename(zip),
    bytes: fs.statSync(zip).size,
    sha256: sha256(zip),
    label,
    source_commit: buildCommit,
    files: files.length + 1,
  };
  fs.writeFileSync(`${zip}.sha256`, `${zipInfo.sha256}  ${zipInfo.zip}\n`);
  fs.writeFileSync(`${zip}.json`, JSON.stringify(zipInfo, null, 2) + "\n");
}
console.log(
  JSON.stringify({
    out,
    zip,
    zip_sha256: zipInfo?.sha256 ?? null,
    zip_bytes: zipInfo?.bytes ?? null,
    files: files.length + 1,
    tests: ids.length,
  }),
);

// The tester's own check, written into the kit (Node built-ins only).
function verifyScript() {
  return `#!/usr/bin/env node
// Verifies every kit file against SHA256SUMS.txt and prints the package identity.
//   node verify-kit.mjs
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
const dir = import.meta.dirname;
let bad = 0;
for (const line of fs.readFileSync(path.join(dir, "SHA256SUMS.txt"), "utf8").split("\\n").filter(Boolean)) {
  const [want, rel] = [line.slice(0, 64), line.slice(66)];
  const file = path.join(dir, rel);
  const got = fs.existsSync(file) ? createHash("sha256").update(fs.readFileSync(file)).digest("hex") : null;
  if (got !== want) bad++;
  console.log(\`\${got === want ? "OK      " : got ? "MISMATCH" : "MISSING "} \${rel}\`);
}
const info = JSON.parse(fs.readFileSync(path.join(dir, "KIT-INFO.json"), "utf8"));
console.log(\`\\npackage \${info.package.label} (\${info.package.signing}), smoke_feature: \${info.package.smoke_feature}\`);
for (const [n, f] of Object.entries(info.package.files)) console.log(\`  \${n}  \${f.sha256}  \${f.bytes} B\`);
console.log(\`source commit \${info.source.build_commit}; missing platforms: \${info.missing_platforms.join(", ") || "none"}\`);
console.log(bad ? \`\\n\${bad} file(s) do not match: do not use this kit\` : "\\nall files match");
process.exit(bad ? 1 : 0);
`;
}
