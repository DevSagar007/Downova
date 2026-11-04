#!/usr/bin/env node
// Validates a delivered acceptance-kit ZIP (B2.35): the archive itself, not the folder it was built from.
//
//   node scripts/acceptance/validate-kit-zip.mjs --zip dist-packages/acceptance-kit-b2-<label>.zip
//        [--out <dir for kit-validation.json>] [--keep]
//
// 1. Reads the archive (scripts/lib/zip.mjs): every CRC, one top folder, only known kit paths, and
//    exactly the files SHA256SUMS.txt lists (plus SHA256SUMS.txt itself).
// 2. Extracts it with this OS's standard tool into a fresh temporary folder: Windows' bsdtar
//    (System32\tar.exe), macOS `ditto -x -k`, Linux `unzip`. A missing tool is a clear failure.
//    On macOS and Linux the AppImage and verify-kit.mjs must come out executable.
// 3. Runs `node verify-kit.mjs` in the extracted kit.
// 4. Starts both fixtures with the checklist's documented commands (S2, S3), downloads one file from
//    each and compares it with the SHA-256 the fixture announces, then stops and reaps exactly those
//    two processes.
// Writes kit-validation.json; on GitHub Actions also a job-summary section. Exit 1 on any failure.
// This is build and delivery verification only: it is never installed-app or human acceptance.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { readFirstJsonLine } from "../lib/first-line.mjs";
import { readZip } from "../lib/zip.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const zip = path.resolve(arg("--zip") ?? "");
if (!fs.existsSync(zip)) {
  console.error(`validate-kit-zip: --zip ${zip} does not exist`);
  process.exit(2);
}
const outDir = path.resolve(arg("--out", path.dirname(zip)));
const steps = [];
const record = (step, ok, detail = {}) => {
  steps.push({ step, status: ok ? "passed" : "failed", ...detail });
  console.log(`${ok ? "PASSED" : "FAILED"}  ${step} ${JSON.stringify(detail).slice(0, 300)}`);
  return ok;
};
const sha = (b) => createHash("sha256").update(b).digest("hex");
const get = (url) =>
  new Promise((resolve, reject) => {
    const req = http.get(url, (res) => {
      const parts = [];
      res.on("data", (d) => parts.push(d));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(parts) }));
    });
    req.on("error", reject);
    req.setTimeout(120_000, () => req.destroy(new Error(`timeout: ${url}`)));
  });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-kit-validate-"));
const children = [];
let kitDir = null;
try {
  // ---------------------------------------------------------------- 1. archive
  const entries = readZip(zip);
  const tops = new Set(entries.map((e) => e.name.split("/")[0]));
  const top = [...tops][0];
  const names = entries.map((e) => e.name.slice(top.length + 1)).sort();
  const sumsEntry = entries.find((e) => e.name === `${top}/SHA256SUMS.txt`);
  const listed = sumsEntry
    ? sumsEntry.data
        .toString("utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => l.slice(66))
    : [];
  const expected = [...listed, "SHA256SUMS.txt"].sort();
  const forbidden = names.filter((n) => /(^|\/)(node_modules|\.git|target)\//.test(n));
  record("archive: valid ZIP, every CRC checked, one top folder", tops.size === 1, {
    entries: entries.length,
    top,
    bytes: fs.statSync(zip).size,
    sha256: sha(fs.readFileSync(zip)),
  });
  record(
    "archive: exactly the files SHA256SUMS.txt lists; no caches, node_modules or repository files",
    JSON.stringify(names) === JSON.stringify(expected) && forbidden.length === 0,
    {
      extra: names.filter((n) => !expected.includes(n)),
      missing: expected.filter((n) => !names.includes(n)),
      forbidden,
    },
  );

  // ---------------------------------------------------------------- 2. standard extraction
  const extractTo = path.join(work, "extracted");
  fs.mkdirSync(extractTo);
  const tool =
    process.platform === "win32"
      ? [
          path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe"),
          ["-x", "-f", zip, "-C", extractTo],
        ]
      : process.platform === "darwin"
        ? ["ditto", ["-x", "-k", zip, extractTo]]
        : ["unzip", ["-q", zip, "-d", extractTo]];
  const x = spawnSync(tool[0], tool[1], { encoding: "utf8" });
  kitDir = path.join(extractTo, top);
  record(
    `extraction with the standard tool (${path.basename(tool[0])})`,
    x.status === 0 && fs.existsSync(kitDir),
    {
      exit: x.status,
      error: x.error ? String(x.error.message) : (x.stderr ?? "").slice(0, 300),
    },
  );
  if (process.platform !== "win32") {
    const mustExec = names.filter((n) => n === "verify-kit.mjs" || n.endsWith(".AppImage"));
    const notExec = mustExec.filter((n) => (fs.statSync(path.join(kitDir, n)).mode & 0o111) === 0);
    record("extracted: verify-kit.mjs and any AppImage are executable", notExec.length === 0, {
      checked: mustExec,
      not_executable: notExec,
    });
  }

  // ---------------------------------------------------------------- 3. verify-kit
  const v = spawnSync(process.execPath, ["verify-kit.mjs"], { cwd: kitDir, encoding: "utf8" });
  const okLines = (v.stdout.match(/^OK /gm) ?? []).length;
  record(
    "verify-kit.mjs in the extracted kit: every file matches",
    v.status === 0 && okLines === listed.length,
    {
      exit: v.status,
      ok_files: okLines,
      listed: listed.length,
      tail: v.stdout.trim().split("\n").slice(-3).join(" | "),
    },
  );

  // ---------------------------------------------------------------- 4. fixtures (documented commands)
  const start = (args) => {
    const c = spawn(process.execPath, args, { cwd: kitDir, stdio: ["ignore", "pipe", "pipe"] });
    let err = "";
    c.stderr.on("data", (d) => (err = (err + d).slice(-2000)));
    children.push(c);
    return { c, err: () => err };
  };
  // S3: node fixtures/perf/fixture-process.mjs --count 7 --size-mib 256 --port 8766
  const fp = start([
    "fixtures/perf/fixture-process.mjs",
    "--count",
    "7",
    "--size-mib",
    "256",
    "--port",
    "8766",
  ]);
  try {
    const first = await readFirstJsonLine(fp.c, { timeoutMs: 300_000 });
    const file = await get(`${first.base}/f0.bin`);
    record(
      "fixture-process (S3 command): first line announced, f0.bin matches its SHA-256",
      file.status === 200 && sha(file.body) === first.sha256["f0.bin"],
      {
        base: first.base,
        files: Object.keys(first.sha256).length,
        bytes: file.body.length,
      },
    );
  } catch (e) {
    record(
      "fixture-process (S3 command): first line announced, f0.bin matches its SHA-256",
      false,
      {
        error: String(e.message ?? e),
        stderr: fp.err(),
      },
    );
  }
  // S2: node fixtures/acceptance/fixture-server.mjs --port 8765
  const fsrv = start(["fixtures/acceptance/fixture-server.mjs", "--port", "8765"]);
  try {
    let page = null;
    for (let i = 0; i < 300 && !page; i++) {
      page = await get("http://127.0.0.1:8765/").catch(() => null);
      if (!page) await sleep(200);
    }
    const html = page?.body.toString("utf8") ?? "";
    const announced = /public\.bin[\s\S]*?\b([0-9a-f]{64})\b/.exec(html)?.[1] ?? null;
    const pub = await get("http://127.0.0.1:8765/public.bin");
    record(
      "fixture-server (S2 command): page served, public.bin matches the SHA-256 on the page",
      page?.status === 200 && announced !== null && sha(pub.body) === announced,
      {
        bytes: pub.body.length,
        announced,
      },
    );
  } catch (e) {
    record(
      "fixture-server (S2 command): page served, public.bin matches the SHA-256 on the page",
      false,
      {
        error: String(e.message ?? e),
        stderr: fsrv.err(),
      },
    );
  }
} catch (e) {
  record("validation run", false, { error: String(e?.stack ?? e) });
} finally {
  // Stop and reap only the processes this validation started.
  for (const c of children) {
    if (c.exitCode === null && c.signalCode === null) {
      const gone = new Promise((r) => c.once("exit", r));
      c.kill();
      await Promise.race([gone, sleep(10_000)]);
    }
  }
  record(
    "fixtures stopped and reaped",
    children.every((c) => c.exitCode !== null || c.signalCode !== null),
    {
      started: children.length,
    },
  );
  if (!argv.includes("--keep")) {
    try {
      fs.rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
    } catch {
      // a just-ended process may still hold a file on Windows; the folder is in TEMP
    }
  }
}

const failed = steps.filter((s) => s.status === "failed");
const result = {
  zip: path.basename(zip),
  platform: `${process.platform}-${process.arch}`,
  node: process.version,
  validated_at: new Date().toISOString(),
  verdict: failed.length ? "failed" : "passed",
  scope: "build and delivery verification only; not installed-app or human acceptance",
  steps,
};
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "kit-validation.json"), JSON.stringify(result, null, 2) + "\n");
if (process.env.GITHUB_STEP_SUMMARY)
  fs.appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    [
      `### Kit validation: ${result.verdict.toUpperCase()} (${result.platform})`,
      "",
      "| Step | Result |",
      "|---|---|",
      ...steps.map((s) => `| ${s.step} | ${s.status} |`),
      "",
    ].join("\n"),
  );
console.log(`\nverdict: ${result.verdict} (${path.join(outDir, "kit-validation.json")})`);
process.exit(failed.length ? 1 : 0);
