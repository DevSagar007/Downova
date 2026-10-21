#!/usr/bin/env node
// Single verification entry point. Runs every check that is actually available and reports
// unavailable checks explicitly — it never prints success for a check it did not run.
//   pnpm verify           -> exit 1 if any check FAILED; unavailable checks reported as UNAVAILABLE
//   pnpm verify:strict    -> additionally exit 1 if any check was UNAVAILABLE (use in CI)
//   pnpm verify -- --only=rust,lint   -> run a subset (group names below)
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const strict = process.argv.includes("--strict");
const only = process.argv
  .find((a) => a.startsWith("--only="))
  ?.slice(7)
  .split(",");
const isWin = process.platform === "win32";

const bin = (pkgDir, name) => {
  const p = path.join(root, pkgDir, "node_modules", ".bin", isWin ? `${name}.cmd` : name);
  return existsSync(p) ? p : null;
};
const onPath = (cmd) =>
  spawnSync(cmd, ["--version"], { shell: isWin, stdio: "ignore" }).status === 0;

const desktop = "apps/desktop";
const contracts = "packages/contracts";
const hasCargo = onPath("cargo");

/** @type {{group: string, name: string, cwd?: string, cmd: string | null, args: string[], unavailable?: string}[]} */
const steps = [
  {
    group: "structure",
    name: "structure checks",
    cmd: process.execPath,
    args: ["scripts/check-structure.mjs"],
  },
  {
    group: "contracts",
    name: "contracts up to date + examples",
    cmd: process.execPath,
    args: ["scripts/gen-contracts.mjs", "--check"],
  },
  { group: "format", name: "prettier --check", cmd: bin(".", "prettier"), args: ["--check", "."] },
  { group: "lint", name: "eslint", cmd: bin(".", "eslint"), args: [".", "--max-warnings=0"] },
  {
    group: "typecheck",
    name: "tsc contracts",
    cwd: contracts,
    cmd: bin(contracts, "tsc"),
    args: ["-p", "tsconfig.json"],
  },
  { group: "typecheck", name: "tsc desktop", cwd: desktop, cmd: bin(desktop, "tsc"), args: ["-b"] },
  {
    group: "test",
    name: "vitest desktop",
    cwd: desktop,
    cmd: bin(desktop, "vitest"),
    args: ["run"],
  },
  {
    group: "build",
    name: "vite build desktop web",
    cwd: desktop,
    cmd: bin(desktop, "vite"),
    args: ["build"],
  },
  ...[
    { name: "cargo fmt --check", args: ["fmt", "--all", "--check"] },
    {
      name: "cargo clippy",
      args: ["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
    },
    { name: "cargo test", args: ["test", "--workspace", "--locked"] },
  ].map((s) => ({
    group: "rust",
    ...s,
    cmd: hasCargo ? "cargo" : null,
    unavailable: "cargo not found on PATH (install Rust via rustup; see docs/setup.md)",
  })),
];

const results = [];
for (const step of steps) {
  if (only && !only.includes(step.group)) continue;
  const label = `[${step.group}] ${step.name}`;
  if (!step.cmd) {
    const reason = step.unavailable ?? "tool not installed; run `pnpm install`";
    console.log(`\n--- ${label}: UNAVAILABLE — ${reason}`);
    results.push({ label, status: "UNAVAILABLE", reason });
    continue;
  }
  console.log(`\n--- ${label}`);
  const started = Date.now();
  const r = spawnSync(step.cmd, step.args, {
    cwd: path.join(root, step.cwd ?? "."),
    stdio: "inherit",
    shell: isWin && step.cmd.endsWith(".cmd"),
  });
  const status = r.status === 0 ? "PASSED" : "FAILED";
  results.push({ label, status, seconds: ((Date.now() - started) / 1000).toFixed(1) });
}

console.log("\n=== verify summary ===");
for (const r of results)
  console.log(
    `${r.status.padEnd(11)} ${r.label}${r.seconds ? ` (${r.seconds}s)` : ""}${r.reason ? ` — ${r.reason}` : ""}`,
  );
const count = (s) => results.filter((r) => r.status === s).length;
const failed = count("FAILED");
const unavailable = count("UNAVAILABLE");
const overall = failed ? "FAILED" : unavailable ? "PARTIAL (some checks did not run)" : "PASSED";
console.log(
  `\n${count("PASSED")} passed, ${failed} failed, ${unavailable} unavailable — result: ${overall}`,
);
process.exit(failed || (strict && unavailable) ? 1 : 0);
