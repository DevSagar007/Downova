#!/usr/bin/env node
// Builds the B2 installed-app acceptance kit from a NORMAL package (B2.34).
//
//   node scripts/acceptance/build-kit.mjs [--package-dir dist-packages/windows-x86_64]
//        [--out dist-packages/acceptance-kit-b2-<label>] [--zip] [--skip-source-check]
//
// What it does, and refuses:
// - Reads the package's build-info.json; refuses a smoke-feature (test) package.
// - Verifies the installers against the package's SHA256SUMS.txt and every extension file against
//   EXTENSION-SHA256SUMS.txt; any mismatch or missing file stops it.
// - Compares the package's source commit with HEAD over the production paths. A change outside test
//   files stops it (rebuild the package first); test-only changes are listed in KIT-INFO.json.
//   `--skip-source-check` is for the unit test only (no git repository there).
// - Copies the installers, the production extension, the fixture scripts with their one relative
//   import (`lib/fixture.mjs`, which needs only Node built-ins), the checklist, the Bangla quick-start.
// - Generates RESULTS.csv (one row per test id in the checklist), KIT-INFO.json, verify-kit.mjs, an
//   empty evidence/ folder, and SHA256SUMS.txt over every kit file. Large fixture data is generated at
//   run time by the fixture scripts, never stored in the kit.
// - `--zip` also writes <out>.zip with the platform's `tar -a` (Windows 10+ and macOS bsdtar).
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

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
const sums = readSums(path.join(packageDir, "SHA256SUMS.txt"));
const installers = Object.keys(sums);
if (!installers.length) fail("SHA256SUMS.txt lists no installer");
for (const name of installers) {
  const file = path.join(packageDir, name);
  if (!fs.existsSync(file)) fail(`missing ${name}`);
  if (sha256(file) !== sums[name]) fail(`${name} does not match SHA256SUMS.txt`);
}
const extDir = path.join(packageDir, "extension-unpacked");
const extSums = readSums(path.join(packageDir, "EXTENSION-SHA256SUMS.txt"));
for (const [rel, want] of Object.entries(extSums)) {
  const file = path.join(extDir, rel);
  if (!fs.existsSync(file)) fail(`extension file missing: ${rel}`);
  if (sha256(file) !== want) fail(`extension file ${rel} does not match EXTENSION-SHA256SUMS.txt`);
}
const extManifest = JSON.parse(fs.readFileSync(path.join(extDir, "manifest.json"), "utf8"));
if (!extManifest.key) fail("the extension has no manifest key (its ID would not match the helper)");

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
copy(path.join(root, "docs/acceptance/b2-quickstart-bn.md"), "QUICKSTART-bn.md");
fs.mkdirSync(path.join(out, "evidence"), { recursive: true });
fs.writeFileSync(
  path.join(out, "evidence", "README.txt"),
  "Put screenshots, logs and fixture-hashes.json here; name each file in RESULTS.csv.\n",
);

// RESULTS.csv: one row per test id, in checklist order.
const checklist = fs.readFileSync(path.join(root, "docs/acceptance/b2-checklist.md"), "utf8");
const ids = [...checklist.matchAll(/^\| ([A-Z]\d+) \|/gm)].map((m) => m[1]);
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
const setup = installers.find((n) => /setup\.exe$/.test(n)) ?? installers[0];
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
    files: Object.fromEntries(
      installers.map((n) => [
        n,
        { sha256: sums[n], bytes: fs.statSync(path.join(packageDir, n)).size },
      ]),
    ),
    setup_sha256: sums[setup],
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
fs.writeFileSync(
  path.join(out, "SHA256SUMS.txt"),
  files.map((f) => `${sha256(path.join(out, f))}  ${f}`).join("\n") + "\n",
);

let zip = null;
if (argv.includes("--zip")) {
  zip = `${out}.zip`;
  fs.rmSync(zip, { force: true });
  // Windows' own bsdtar (System32): a GNU tar earlier on PATH (Git Bash) cannot write zip files.
  const tar =
    process.platform === "win32"
      ? path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe")
      : "tar";
  const r = spawnSync(tar, ["-a", "-c", "-f", zip, "-C", path.dirname(out), path.basename(out)], {
    encoding: "utf8",
  });
  if (r.status !== 0) fail(`zip failed: ${r.stderr}`);
}
console.log(
  JSON.stringify({
    out,
    zip,
    zip_sha256: zip ? sha256(zip) : null,
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
