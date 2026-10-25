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
const extension = "apps/extension";
const hasCargo = onPath("cargo");
// Same lookup order as crates/backend-aria2/src/binary.rs.
const aria2Exe = isWin ? "aria2c.exe" : "aria2c";
const aria2 = [
  process.env.DM_ARIA2_PATH,
  path.join(root, "tools", "aria2", aria2Exe),
  ...(process.env.PATH ?? "").split(path.delimiter).map((dir) => path.join(dir, aria2Exe)),
].find((p) => p && existsSync(p));

/** @type {{group: string, name: string, cwd?: string, cmd: string | null, args: string[], unavailable?: string, timeoutMs?: number}[]} */
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
    group: "typecheck",
    name: "tsc extension",
    cwd: extension,
    cmd: bin(extension, "tsc"),
    args: ["-p", "tsconfig.json"],
  },
  {
    group: "test",
    name: "vitest desktop",
    cwd: desktop,
    cmd: bin(desktop, "vitest"),
    args: ["run"],
  },
  {
    group: "test",
    name: "vitest extension",
    cwd: extension,
    cmd: bin(extension, "vitest"),
    args: ["run"],
  },
  {
    group: "build",
    name: "vite build extension",
    cwd: extension,
    cmd: bin(extension, "vite"),
    args: ["build"],
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
    {
      name: "native host end-to-end (dm-browser-host + dm-service, --ignored)",
      // Bounded: the test itself waits at most ~2 min; the rest is building dm-service.
      timeoutMs: 15 * 60 * 1000,
      args: [
        "test",
        "-p",
        "dm-browser-host",
        "--locked",
        "--test",
        "host",
        "--",
        "--ignored",
        "--nocapture",
      ],
    },
  ].map((s) => ({
    group: "rust",
    ...s,
    cmd: hasCargo ? "cargo" : null,
    unavailable: "cargo not found on PATH (install Rust via rustup; see docs/setup.md)",
  })),
  {
    group: "aria2",
    name: "aria2 fixture tests (dm-backend-aria2, --ignored)",
    args: [
      "test",
      "-p",
      "dm-backend-aria2",
      "--locked",
      "--test",
      "fixtures",
      "--",
      "--ignored",
      "--nocapture",
      "--test-threads=2",
    ],
    cmd: hasCargo && aria2 ? "cargo" : null,
    unavailable: !hasCargo
      ? "cargo not found on PATH"
      : "aria2c not found (DM_ARIA2_PATH, tools/aria2/, or PATH; run node scripts/fetch-aria2.mjs on Windows)",
  },
  {
    group: "aria2",
    name: "HTTP correctness fixtures (dm-backend-aria2 http_correctness, --ignored)",
    args: [
      "test",
      "-p",
      "dm-backend-aria2",
      "--locked",
      "--test",
      "http_correctness",
      "--",
      "--ignored",
      "--nocapture",
      "--test-threads=2",
    ],
    cmd: hasCargo && aria2 ? "cargo" : null,
    unavailable: !hasCargo
      ? "cargo not found on PATH"
      : "aria2c not found (DM_ARIA2_PATH, tools/aria2/, or PATH; run node scripts/fetch-aria2.mjs on Windows)",
  },
  {
    group: "aria2",
    name: "service process tests (real dm-service + aria2, --ignored)",
    args: [
      "test",
      "-p",
      "dm-service",
      "--locked",
      "--test",
      "process",
      "--",
      "--ignored",
      "--nocapture",
    ],
    cmd: hasCargo && aria2 ? "cargo" : null,
    unavailable: !hasCargo
      ? "cargo not found on PATH"
      : "aria2c not found (DM_ARIA2_PATH, tools/aria2/, or PATH; run node scripts/fetch-aria2.mjs on Windows)",
  },
  {
    group: "aria2",
    name: "CLI against real service (dm + dm-service + aria2, --ignored)",
    args: [
      "test",
      "-p",
      "dm-cli",
      "--locked",
      "--test",
      "real_service",
      "--",
      "--ignored",
      "--nocapture",
    ],
    cmd: hasCargo && aria2 ? "cargo" : null,
    unavailable: !hasCargo
      ? "cargo not found on PATH"
      : "aria2c not found (DM_ARIA2_PATH, tools/aria2/, or PATH; run node scripts/fetch-aria2.mjs on Windows)",
  },
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
    timeout: step.timeoutMs,
    killSignal: "SIGKILL",
  });
  const timedOut = r.error?.code === "ETIMEDOUT";
  const status = r.status === 0 ? "PASSED" : "FAILED";
  results.push({
    label,
    status,
    seconds: ((Date.now() - started) / 1000).toFixed(1),
    ...(timedOut ? { reason: `timed out after ${step.timeoutMs / 1000} s` } : {}),
  });
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
