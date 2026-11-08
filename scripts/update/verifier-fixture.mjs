#!/usr/bin/env node
// ADV09 / ADR 0030: the real updater verifier against disposable keys.
//
//   node scripts/update/verifier-fixture.mjs [--release]
//
// `--release` runs the test in a release build, where the plugin itself refuses plain HTTP without
// the fixture-only transport flag (case 10; debug builds only warn).
//
// 1. Generates two throw-away minisign key pairs with the pinned Tauri CLI (`tauri signer
//    generate`) in a fresh temporary folder (never in the repository, never reused).
// 2. Signs a fixture artifact with `tauri signer sign` the way releases are signed: bound to the
//    version (`--app-version`), with the wrong key, without a signed version, and for another
//    version than the one announced.
// 3. Runs the ignored Rust test `apps/desktop/src-tauri/tests/update_verifier.rs`, which serves
//    these cases over loopback HTTP and drives the real `tauri-plugin-updater` check, download and
//    verification (plus a tampered artifact, an older version, a wrong package format and an
//    interrupted download).
// 4. Deletes the folder, private keys included, whatever the outcome.
//
// Nothing here is a production key or a release; the public key is trusted only by the test.
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../..");
const isWin = process.platform === "win32";
const tauri = path.join(root, "apps/desktop/node_modules/.bin", isWin ? "tauri.cmd" : "tauri");
if (!fs.existsSync(tauri)) {
  console.error(`missing ${tauri}: run pnpm install first`);
  process.exit(2);
}
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-update-fixture-"));
const run = (args, extraEnv = {}) => {
  const r = spawnSync(tauri, args, {
    cwd: dir,
    encoding: "utf8",
    shell: isWin,
    env: { ...process.env, CI: "true", ...extraEnv },
  });
  if (r.status !== 0)
    throw new Error(`tauri ${args[0]} ${args[1]} failed: ${r.stderr || r.stdout}`);
  return r.stdout;
};
// A per-run password for the disposable keys (an empty one cannot pass through the Windows shell).
const password = randomBytes(16).toString("hex");
let code = 1;
try {
  for (const k of ["trusted", "other"]) {
    run(["signer", "generate", "--ci", "-p", password, "-w", path.join(dir, `${k}.key`)]);
  }
  // The artifact: deterministic bytes standing in for an installer (the verifier signs bytes).
  const artifact = Buffer.alloc(256 * 1024);
  for (let i = 0; i < artifact.length; i++) artifact[i] = (i * 2654435761) >>> 24;
  const artifactPath = path.join(dir, "Downova_0.2.0_x64-setup.exe");
  fs.writeFileSync(artifactPath, artifact);
  const sign = (key, version) => {
    const args = ["signer", "sign", "-f", path.join(dir, `${key}.key`), "-p", password];
    if (version) args.push("--app-version", version);
    args.push(artifactPath);
    run(args);
    const sig = fs.readFileSync(`${artifactPath}.sig`, "utf8").trim();
    fs.rmSync(`${artifactPath}.sig`);
    return sig;
  };
  const cases = {
    pubkey: fs.readFileSync(path.join(dir, "trusted.key.pub"), "utf8").trim(),
    artifact: artifactPath,
    artifact_sha256: createHash("sha256").update(artifact).digest("hex"),
    valid: sign("trusted", "0.2.0"),
    wrong_key: sign("other", "0.2.0"),
    unbound: sign("trusted", null),
    other_version: sign("trusted", "0.1.9"),
  };
  fs.writeFileSync(path.join(dir, "cases.json"), JSON.stringify(cases, null, 2));
  console.log(`fixture: ${dir} (disposable keys; deleted afterwards)`);
  const test = spawnSync(
    "cargo",
    [
      "test",
      ...(process.argv.includes("--release") ? ["--release"] : []),
      "-p",
      "dm-desktop",
      "--test",
      "update_verifier",
      "--",
      "--ignored",
      "--nocapture",
    ],
    {
      cwd: root,
      stdio: "inherit",
      env: { ...process.env, DOWNOVA_UPDATE_FIXTURE: path.join(dir, "cases.json") },
    },
  );
  code = test.status ?? 1;
} catch (e) {
  console.error(String(e?.message ?? e));
} finally {
  fs.rmSync(dir, { recursive: true, force: true });
  console.log(`removed ${dir}`);
}
process.exit(code);
