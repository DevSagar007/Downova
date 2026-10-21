#!/usr/bin/env node
// Lightweight structural validation of the repository. Exits non-zero on any failure.
// Checks: required files, JSON/YAML/TOML syntax, local Markdown links, Claude rule/skill
// frontmatter, Rust crate dependency directions, TypeScript cross-boundary imports, and
// requirement <-> traceability consistency. See docs/testing.md#structural-checks.
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import YAML from "yaml";
import { parse as parseToml } from "smol-toml";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];
const warnings = [];
const fail = (msg) => failures.push(msg);
const rel = (p) => path.relative(root, p).split(path.sep).join("/");
const read = (p) => readFileSync(p, "utf8");

const IGNORED_DIRS = new Set(["node_modules", "target", "dist", ".git", "gen", "coverage"]);
function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (IGNORED_DIRS.has(name)) continue;
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}
const files = walk(root);
const byExt = (...exts) => files.filter((f) => exts.some((e) => f.endsWith(e)));

// 1. Required files --------------------------------------------------------------------------
const REQUIRED = [
  "CLAUDE.md",
  "README.md",
  "PROGRESS.md",
  "requirements.md",
  "design.md",
  "docs/architecture.md",
  "docs/project-structure.md",
  "docs/roadmap.md",
  "docs/traceability.md",
  "docs/testing.md",
  "docs/setup.md",
  "docs/decisions/0001-project-foundation.md",
  "docs/research/download-manager-rnd-bn.md",
  ".claude/settings.json",
  ".claude/rules/workflow.md",
  ".claude/rules/architecture.md",
  ".claude/rules/rust.md",
  ".claude/rules/desktop-ui.md",
  ".claude/rules/browser-integration.md",
  ".claude/rules/verification.md",
  ".claude/skills/dm-engine/SKILL.md",
  ".claude/skills/dm-ui/SKILL.md",
  ".claude/skills/dm-browser/SKILL.md",
  ".claude/skills/dm-verify/SKILL.md",
  "packages/contracts/schema/service-api.v1.schema.json",
  "packages/contracts/src/generated/service-api.v1.ts",
  "apps/desktop/src/ui/tokens.css",
  "Cargo.toml",
  "package.json",
  "pnpm-workspace.yaml",
  "pnpm-lock.yaml",
];
for (const f of REQUIRED) if (!existsSync(path.join(root, f))) fail(`missing required file: ${f}`);

// 2. Config syntax ---------------------------------------------------------------------------
for (const f of byExt(".json")) {
  try {
    JSON.parse(read(f));
  } catch (e) {
    fail(`invalid JSON: ${rel(f)}: ${e.message}`);
  }
}
for (const f of byExt(".yaml", ".yml")) {
  // Multi-document YAML is valid (pnpm 12 lockfiles use it).
  for (const doc of YAML.parseAllDocuments(read(f))) {
    for (const err of doc.errors ?? []) fail(`invalid YAML: ${rel(f)}: ${err.message}`);
  }
}
const tomlDocs = new Map();
for (const f of byExt(".toml")) {
  try {
    tomlDocs.set(f, parseToml(read(f)));
  } catch (e) {
    fail(`invalid TOML: ${rel(f)}: ${e.message}`);
  }
}

// 3. Local Markdown links --------------------------------------------------------------------
const markdown = byExt(".md").filter((f) => !rel(f).startsWith("docs/research/"));
const LINK = /\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
for (const f of markdown) {
  const text = read(f).replace(/```[\s\S]*?```/g, "");
  for (const [, target] of text.matchAll(LINK)) {
    if (/^(https?:|mailto:|#)/.test(target)) continue;
    const clean = decodeURI(target.split("#")[0]).replace(/:\d+$/, "");
    if (!clean) continue;
    if (!existsSync(path.resolve(path.dirname(f), clean))) {
      fail(`broken link in ${rel(f)} -> ${target}`);
    }
  }
}

// 4. Claude rules and skills frontmatter -----------------------------------------------------
function frontmatter(file) {
  const m = read(file).match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
  if (!m) return null;
  return YAML.parse(m[1]);
}
const rulesDir = path.join(root, ".claude", "rules");
if (existsSync(rulesDir)) {
  for (const name of readdirSync(rulesDir).filter((n) => n.endsWith(".md"))) {
    const file = path.join(rulesDir, name);
    let fm;
    try {
      fm = frontmatter(file);
    } catch (e) {
      fail(`rule ${name}: frontmatter is not valid YAML: ${e.message}`);
      continue;
    }
    if (fm === null) continue; // unconditional rule
    const allowed = new Set(["paths"]);
    for (const key of Object.keys(fm))
      if (!allowed.has(key)) fail(`rule ${name}: unsupported key "${key}"`);
    if ("paths" in fm) {
      const ok =
        Array.isArray(fm.paths) &&
        fm.paths.length > 0 &&
        fm.paths.every((p) => typeof p === "string" && p.trim());
      if (!ok) fail(`rule ${name}: "paths" must be a non-empty list of glob strings`);
    }
  }
}
const skillsDir = path.join(root, ".claude", "skills");
if (existsSync(skillsDir)) {
  for (const dir of readdirSync(skillsDir)) {
    const file = path.join(skillsDir, dir, "SKILL.md");
    if (!existsSync(file)) {
      fail(`skill ${dir}: missing SKILL.md`);
      continue;
    }
    let fm;
    try {
      fm = frontmatter(file);
    } catch (e) {
      fail(`skill ${dir}: frontmatter is not valid YAML: ${e.message}`);
      continue;
    }
    if (!fm) {
      fail(`skill ${dir}: missing YAML frontmatter`);
      continue;
    }
    if (fm.name !== dir) fail(`skill ${dir}: name "${fm.name}" must equal directory name`);
    if (!/^[a-z0-9-]{1,64}$/.test(String(fm.name)))
      fail(`skill ${dir}: name must be lowercase-hyphenated, <=64 chars`);
    if (
      typeof fm.description !== "string" ||
      fm.description.length < 40 ||
      fm.description.length > 1024
    ) {
      fail(`skill ${dir}: description must be 40-1024 characters and say when to use the skill`);
    }
  }
}

// 5. Rust crate dependency directions ---------------------------------------------------------
// Allowed internal (dm-*) dependencies per crate. Anything else is a boundary violation.
const ALLOWED_DM_DEPS = {
  "dm-domain": [],
  "dm-storage": ["dm-domain"],
  "dm-backend-aria2": ["dm-domain"],
  "dm-platform": ["dm-domain"],
  "dm-service": ["dm-domain", "dm-storage", "dm-backend-aria2", "dm-platform"],
  "dm-cli": ["dm-domain", "dm-platform"],
  "dm-browser-host": ["dm-domain", "dm-platform"],
  "dm-desktop": ["dm-domain", "dm-platform"],
};
const workspace = tomlDocs.get(path.join(root, "Cargo.toml"));
for (const member of workspace?.workspace?.members ?? []) {
  const manifestPath = path.join(root, member, "Cargo.toml");
  const manifest = tomlDocs.get(manifestPath);
  if (!manifest) {
    fail(`workspace member ${member} has no parseable Cargo.toml`);
    continue;
  }
  const crate = manifest.package?.name;
  if (!(crate in ALLOWED_DM_DEPS)) {
    fail(
      `${member}: crate "${crate}" is not registered in docs/project-structure.md dependency table (update check-structure.mjs too)`,
    );
    continue;
  }
  const deps = Object.keys({ ...manifest.dependencies, ...manifest["build-dependencies"] });
  for (const dep of deps.filter((d) => d.startsWith("dm-"))) {
    if (!ALLOWED_DM_DEPS[crate].includes(dep))
      fail(`${crate} must not depend on ${dep} (see docs/project-structure.md)`);
  }
  if (crate !== "dm-backend-aria2" && deps.some((d) => /aria2/i.test(d))) {
    fail(`${crate}: only dm-backend-aria2 may depend on aria2-specific crates`);
  }
}

// 6. TypeScript cross-boundary imports -------------------------------------------------------
const IMPORT =
  /(?:import|export)\s[^'"]*?from\s+["']([^"']+)["']|import\(\s*["']([^"']+)["']\s*\)/g;
for (const f of byExt(".ts", ".tsx").filter((f) => !f.endsWith(".d.ts"))) {
  const r = rel(f);
  for (const m of read(f).matchAll(IMPORT)) {
    const spec = m[1] ?? m[2];
    if (spec.startsWith(".")) {
      const target = rel(path.resolve(path.dirname(f), spec));
      const topOf = (p) => p.split("/").slice(0, 2).join("/");
      if (topOf(target) !== topOf(r) && !target.startsWith("packages/contracts")) {
        fail(
          `${r}: relative import "${spec}" crosses a package boundary; use a workspace package instead`,
        );
      }
    }
    if (r.startsWith("packages/contracts/") && spec.startsWith("@dm/"))
      fail(`${r}: contracts must not import other workspace packages`);
    if (
      r.startsWith("apps/desktop/src/") &&
      spec.startsWith("@tauri-apps/") &&
      !r.startsWith("apps/desktop/src/services/")
    ) {
      fail(`${r}: only src/services/ may import ${spec} (service-client boundary)`);
    }
  }
}

// 7. Requirements <-> traceability -----------------------------------------------------------
const STATUSES = new Set([
  "documented",
  "in-progress",
  "implemented",
  "verified",
  "blocked",
  "deferred",
  "proposed",
]);
const reqText = existsSync(path.join(root, "requirements.md"))
  ? read(path.join(root, "requirements.md"))
  : "";
const reqIds = new Map();
const blocks = reqText.split(/^### /m).slice(1);
for (const block of blocks) {
  const id = block.match(/^([A-Z]+[0-9]*-?[0-9]+)\s/)?.[1];
  if (!id) continue;
  if (reqIds.has(id)) fail(`requirements.md: duplicate requirement ID ${id}`);
  const status = block.match(/^- \*\*Status:\*\*\s*`([a-z-]+)`/m)?.[1];
  for (const field of ["Outcome", "Priority", "Depends on", "Acceptance", "Source", "Status"]) {
    if (!new RegExp(`^- \\*\\*${field}:\\*\\*`, "m").test(block))
      fail(`requirements.md: ${id} lacks "${field}"`);
  }
  if (status && !STATUSES.has(status))
    fail(`requirements.md: ${id} has unknown status "${status}"`);
  reqIds.set(id, status);
}
if (reqIds.size === 0) fail("requirements.md: no requirements found");
const traceText = existsSync(path.join(root, "docs/traceability.md"))
  ? read(path.join(root, "docs/traceability.md"))
  : "";
const traced = new Set(
  [...traceText.matchAll(/^\|\s*([A-Z]+[0-9]*-?[0-9]+)\s*\|/gm)].map((m) => m[1]),
);
for (const id of reqIds.keys())
  if (!traced.has(id)) fail(`docs/traceability.md: missing row for ${id}`);
for (const id of traced)
  if (!reqIds.has(id)) fail(`docs/traceability.md: ${id} is not defined in requirements.md`);
for (const [id, status] of reqIds) {
  if (status === "verified") {
    const row = traceText.split("\n").find((l) => l.startsWith(`| ${id} `));
    const evidence = row?.split("|").map((c) => c.trim())[5];
    if (!evidence || evidence === "—" || evidence === "-")
      fail(`${id} is "verified" but has no verification evidence in docs/traceability.md`);
  }
}

// 8. Instruction size --------------------------------------------------------------------------
const claudeLines = existsSync(path.join(root, "CLAUDE.md"))
  ? read(path.join(root, "CLAUDE.md")).split("\n").length
  : 0;
if (claudeLines > 150) warnings.push(`CLAUDE.md has ${claudeLines} lines (target < 150)`);

for (const w of warnings) console.warn(`structure: WARN ${w}`);
if (failures.length) {
  for (const f of failures) console.error(`structure: FAIL ${f}`);
  console.error(`structure: ${failures.length} failure(s)`);
  process.exit(1);
}
console.log(
  `structure: OK (${files.length} files scanned, ${reqIds.size} requirements traced, ${markdown.length} Markdown files link-checked)`,
);
