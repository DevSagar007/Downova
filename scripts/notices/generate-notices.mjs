#!/usr/bin/env node
// REL-03: reproducible inventory of what Downova packages redistribute, with license texts taken
// from the packages themselves (never guessed).
//
//   node scripts/notices/generate-notices.mjs           # writes the inventory
//   node scripts/notices/generate-notices.mjs --check   # exit 1 if the checked-in file is stale
//
// Sources:
// - Rust: `cargo metadata --locked --filter-platform <triple>` for each supported target, walked
//   from the shipped binaries (scripts/lib/notices.mjs `classifyCargo`); texts are the top-level
//   LICENSE/COPYING/NOTICE files of each crate's published source in the Cargo registry.
// - Web: the desktop's production dependency closure (apps/desktop/package.json `dependencies`,
//   resolved through node_modules); the extension has no runtime dependencies.
// - Other redistributed components (Rust std, installer runtimes): third_party/notices/manifest.json.
// Output: apps/desktop/src/generated/third-party.notices.json (read by Help → Third-party notices
// and rendered to THIRD-PARTY-NOTICES.txt by scripts/package.mjs).
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import {
  NOTICE_FILE,
  TARGETS,
  checkInventory,
  classifyCargo,
  normalizeText,
  textId,
  unresolvedFor,
} from "../lib/notices.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const out = path.join(root, "apps/desktop/src/generated/third-party.notices.json");
const check = process.argv.includes("--check");
const sha = (p) => crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");

const texts = {};
function addText(file, text, user) {
  const id = textId(text);
  texts[id] ??= { file, text: normalizeText(text), used_by: [] };
  if (!texts[id].used_by.includes(user)) texts[id].used_by.push(user);
  return id;
}
function noticeFiles(dir, extra) {
  const names = fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && NOTICE_FILE.test(e.name))
    .map((e) => e.name);
  if (extra && !names.includes(extra) && fs.existsSync(path.join(dir, extra))) names.push(extra);
  return names.sort();
}

// ---------------------------------------------------------------- upstream overrides
// third_party/notices/overrides.json: { crates: [{ name, version, revision, path_in_vcs,
// provenance, files: [{ path, source_url, sha256 }] }] }. Each file is read from
// third_party/notices/<path> and must match its recorded sha256; an override that matches no
// distributed name@version, or that is not needed, fails the generation (stale or mismatched).
const overridesPath = path.join(root, "third_party/notices/overrides.json");
const overrideDoc = fs.existsSync(overridesPath)
  ? JSON.parse(fs.readFileSync(overridesPath, "utf8"))
  : { crates: [] };
const overrides = new Map(overrideDoc.crates.map((o) => [`${o.name}@${o.version}`, o]));
const usedOverrides = new Set();
const overrideProblems = [];
function overrideFor(name, version) {
  const o = overrides.get(`${name}@${version}`);
  if (!o) return null;
  const files = [];
  for (const f of o.files ?? []) {
    const file = path.join(root, "third_party/notices", f.path);
    if (!fs.existsSync(file)) {
      overrideProblems.push(`${name} ${version}: missing ${f.path}`);
      continue;
    }
    const bytes = fs.readFileSync(file);
    if (crypto.createHash("sha256").update(bytes).digest("hex") !== f.sha256) {
      overrideProblems.push(`${name} ${version}: ${f.path} does not match its recorded sha256`);
      continue;
    }
    files.push({ ...f, text: bytes.toString("utf8") });
  }
  return files.length > 0 ? { ...o, files } : null;
}

// ---------------------------------------------------------------- Rust
const cargo = new Map(); // id -> { pkg, targets:Set, buildOnly:Set }
let allPackages = new Map();
for (const t of TARGETS) {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--locked", "--filter-platform", t.triple],
      { cwd: root, encoding: "utf8", maxBuffer: 512 * 1024 * 1024 },
    ),
  );
  for (const p of metadata.packages) allPackages.set(p.id, p);
  const { distributed, buildOnly } = classifyCargo(metadata);
  for (const id of distributed) {
    const e = cargo.get(id) ?? { targets: new Set(), buildOnly: new Set() };
    e.targets.add(t.id);
    cargo.set(id, e);
  }
  for (const id of buildOnly) {
    const e = cargo.get(id) ?? { targets: new Set(), buildOnly: new Set() };
    e.buildOnly.add(t.id);
    cargo.set(id, e);
  }
}
const components = [];
const excluded = [];
const members = new Set(
  JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--no-deps"], {
      cwd: root,
      encoding: "utf8",
    }),
  ).packages.map((p) => p.name),
);
for (const [id, p] of allPackages) {
  if (members.has(p.name)) continue;
  const e = cargo.get(id);
  const label = `${p.name} ${p.version}`;
  if (!e || e.targets.size === 0) {
    excluded.push({
      name: p.name,
      version: p.version,
      ecosystem: "cargo",
      class: e?.buildOnly.size ? "build-only" : "not-distributed",
      reason: e?.buildOnly.size
        ? "build script, procedural macro or their dependencies; not linked into a shipped binary"
        : "only used by tests, development tools or targets Downova does not ship",
    });
    continue;
  }
  const dir = path.dirname(p.manifest_path);
  const files = noticeFiles(dir, p.license_file ?? undefined);
  const ids = files.map((f) =>
    addText(`${p.name}-${p.version}/${f}`, fs.readFileSync(path.join(dir, f), "utf8"), label),
  );
  // A published crate without license files: texts from the exact upstream revision it was
  // published from (third_party/notices/overrides.json, version-bound and checksummed).
  const override = files.length === 0 ? overrideFor(p.name, p.version) : null;
  if (files.length > 0 && overrides.has(`${p.name}@${p.version}`))
    overrideProblems.push(
      `${p.name} ${p.version} ships its own license files; remove its override`,
    );
  if (override) {
    usedOverrides.add(`${p.name}@${p.version}`);
    for (const f of override.files) ids.push(addText(f.path, f.text, label));
  }
  const c = {
    name: p.name,
    version: p.version,
    ecosystem: "cargo",
    license: p.license ?? (p.license_file ? `see ${p.license_file}` : null),
    source: p.source ?? "local",
    repository: p.repository ?? null,
    targets: TARGETS.map((t) => t.id).filter((t) => e.targets.has(t)),
    texts: ids,
    ...(override
      ? {
          text_provenance: {
            revision: override.revision,
            path_in_vcs: override.path_in_vcs ?? null,
            provenance: override.provenance,
            files: override.files.map((f) => ({
              path: f.path,
              source_url: f.source_url,
              sha256: f.sha256,
            })),
          },
        }
      : {}),
  };
  c.unresolved = unresolvedFor(c);
  if (override && override.complete === false)
    c.unresolved.push(
      `the exact upstream revision (${override.revision.slice(0, 12)}) has only a licensing statement, no license text or copyright notice`,
    );
  components.push(c);
}

// ---------------------------------------------------------------- web bundle
function findPackageDir(fromDir, name) {
  for (let d = fromDir; ; d = path.dirname(d)) {
    if (path.basename(d) === "node_modules") {
      const p = path.join(d, name);
      if (fs.existsSync(path.join(p, "package.json"))) return fs.realpathSync(p);
    }
    const nm = path.join(d, "node_modules", name);
    if (fs.existsSync(path.join(nm, "package.json"))) return fs.realpathSync(nm);
    if (path.dirname(d) === d) return null;
  }
}
const desktopDir = path.join(root, "apps/desktop");
const desktopPkg = JSON.parse(fs.readFileSync(path.join(desktopDir, "package.json"), "utf8"));
const queue = Object.entries(desktopPkg.dependencies)
  .filter(([, v]) => !String(v).startsWith("workspace:"))
  .map(([n]) => [n, desktopDir]);
const npmSeen = new Set();
while (queue.length > 0) {
  const [name, from] = queue.shift();
  const dir = findPackageDir(from, name);
  if (!dir) throw new Error(`cannot resolve ${name} from ${from}; run pnpm install`);
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  const key = `${pkg.name}@${pkg.version}`;
  if (npmSeen.has(key)) continue;
  npmSeen.add(key);
  for (const d of Object.keys(pkg.dependencies ?? {})) queue.push([d, dir]);
  const files = noticeFiles(dir);
  const ids = files.map((f) =>
    addText(`${pkg.name}-${pkg.version}/${f}`, fs.readFileSync(path.join(dir, f), "utf8"), key),
  );
  const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
  const c = {
    name: pkg.name,
    version: pkg.version,
    ecosystem: "npm",
    license: typeof pkg.license === "string" ? pkg.license : null,
    source: "npm registry (pnpm-lock.yaml)",
    repository: repo ?? null,
    targets: TARGETS.map((t) => t.id),
    texts: ids,
  };
  c.unresolved = unresolvedFor(c);
  components.push(c);
}

// ---------------------------------------------------------------- other redistributed components
const manifestPath = path.join(root, "third_party/notices/manifest.json");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
for (const m of manifest.components) {
  const label = `${m.name}${m.version ? ` ${m.version}` : ""}`;
  const ids = m.files.map((f) =>
    addText(f, fs.readFileSync(path.join(root, "third_party/notices", f), "utf8"), label),
  );
  components.push({
    name: m.name,
    version: m.version,
    ecosystem: m.ecosystem,
    license: m.license,
    source: m.source,
    repository: m.repository,
    targets: m.targets,
    texts: ids,
    unresolved: [
      ...m.unresolved,
      ...unresolvedFor({ license: m.license, texts: ids }).filter((u) => !m.unresolved.includes(u)),
    ],
  });
}

const order = { toolchain: 0, installer: 1, cargo: 2, npm: 3 };
components.sort(
  (a, b) =>
    order[a.ecosystem] - order[b.ecosystem] ||
    a.name.localeCompare(b.name, "en") ||
    String(a.version).localeCompare(String(b.version), "en"),
);
excluded.sort(
  (a, b) => a.name.localeCompare(b.name, "en") || a.version.localeCompare(b.version, "en"),
);
for (const t of Object.values(texts)) t.used_by.sort((a, b) => a.localeCompare(b, "en"));
const sortedTexts = Object.fromEntries(
  Object.entries(texts).sort(([a], [b]) => a.localeCompare(b)),
);

const data = {
  schema: 1,
  inputs: {
    cargo_lock_sha256: sha(path.join(root, "Cargo.lock")),
    pnpm_lock_sha256: sha(path.join(root, "pnpm-lock.yaml")),
    manifest_sha256: sha(manifestPath),
  },
  targets: TARGETS.map((t) => t.id),
  summary: {
    distributed: components.length,
    unresolved: components.filter((c) => c.unresolved.length > 0).length,
    excluded: excluded.length,
  },
  components,
  external: manifest.external,
  excluded,
  texts: sortedTexts,
};
for (const key of overrides.keys())
  if (!usedOverrides.has(key) && !overrideProblems.some((p) => p.startsWith(key.replace("@", " "))))
    overrideProblems.push(`stale override ${key}: no distributed crate needs it`);
const problems = [...overrideProblems, ...checkInventory(data)];
if (problems.length > 0) {
  for (const p of problems) console.error(`notices: ${p}`);
  process.exit(1);
}
const json = JSON.stringify(data, null, 1) + "\n";
if (check) {
  const current = fs.existsSync(out) ? fs.readFileSync(out, "utf8") : "";
  if (current !== json) {
    console.error(
      "notices: apps/desktop/src/generated/third-party.notices.json is stale; run node scripts/notices/generate-notices.mjs",
    );
    process.exit(1);
  }
  console.log(
    `notices: up to date (${data.summary.distributed} distributed components, ${data.summary.unresolved} with unresolved metadata)`,
  );
} else {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, json);
  console.log(
    `notices: ${data.summary.distributed} distributed components (${data.summary.unresolved} with unresolved metadata), ${data.summary.excluded} excluded, ${Object.keys(texts).length} distinct texts → ${path.relative(root, out)} (${Math.round(json.length / 1024)} KiB)`,
  );
}
