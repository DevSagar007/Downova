#!/usr/bin/env node
// B3.3 packaged exercise of SEC-01, SEC-02 and BRW-05 (ADR 0027) on the NORMAL Windows package.
//
//   node scripts/acceptance/secure-auth-acceptance.mjs --package-dir dist-packages/windows-x86_64
//
// Windows only. The MSI is extracted with `msiexec /a` (nothing is installed). The packaged release
// `dm-service` runs with a temporary data folder and therefore uses the REAL Windows Credential
// Manager (release builds have no test switches). The packaged `dm-browser-host` is driven exactly
// as Chrome drives it (caller origin argument, length-prefixed JSON); `dm` observes.
//
// Rows:
//   Q1 a public link whose query carries a synthetic secret completes (SHA-256); the data key is
//      created in Credential Manager under this data folder's "Downova/…" namespace; no file in the
//      data folder holds the secret.
//   Q2 a signed-in hand-off with "remember" through the packaged host is accepted; the snapshot and
//      `dm credentials` show the site, browser and "remembered" only; the host's stdout/stderr and
//      every data file are free of the cookie; a Credential Manager entry exists for the sign-in.
//   Q3 the same hand-off for a link of another origin, and for http, is refused by the host
//      (AUTH_SCOPE_UNSUPPORTED) and creates no job.
//   Q4 `dm credentials-forget` removes the sign-in from the service and from Credential Manager and
//      pauses its download.
//   Q5 after Forget, the held download is listed by the packaged host and reconnected (release
//      build, real Credential Manager), keeping its pause.
//   A signed-in TRANSFER is not exercised here: a release build trusts only the system's roots, and
//   installing a test CA would change the system. It is covered by the debug end-to-end tests.
// Every Credential Manager entry this run created is deleted at the end.
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { payload, startFixtureServer } from "../lib/fixture.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const packageDir = path.resolve(arg("--package-dir", "dist-packages/windows-x86_64"));
const buildInfo = JSON.parse(fs.readFileSync(path.join(packageDir, "build-info.json"), "utf8"));
const outDir = path.resolve(
  arg("--out", path.join("acceptance-results", `${buildInfo.label}-secure-auth`)),
);
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (b) => crypto.createHash("sha256").update(b).digest("hex");
const SECRET = "SYNTH-PKG-SECRET-b33";
const COOKIE = "SYNTH-PKG-COOKIE-b33";
const ORIGIN_ARG = "chrome-extension://bkmpnfclpdmcbdgmeakmnoilmkdmdppf/";
const results = {
  started_at: new Date().toISOString(),
  observer: "automated: packaged release service, packaged native host and dm CLI",
  host: `${os.type()} ${os.release()} ${os.arch()}`,
  steps: [],
};
const record = (step, status, detail = {}) => {
  results.steps.push({ ...detail, step, status });
  console.log(`${status.toUpperCase().padEnd(8)} ${step} ${JSON.stringify(detail).slice(0, 600)}`);
};
const finish = (code) => {
  results.finished_at = new Date().toISOString();
  results.verdict = results.steps.some((s) => !["passed", "not run"].includes(s.status))
    ? "failed"
    : "passed";
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(`verdict ${results.verdict}; results in ${outDir}`);
  process.exit(code ?? (results.verdict === "passed" ? 0 : 1));
};
if (process.platform !== "win32") {
  record("platform", "not run", { reason: "this script drives Windows packages only" });
  finish(2);
}
if (buildInfo.smoke_feature) throw new Error("refusing a smoke-test package: use the normal build");
const msi = fs.readdirSync(packageDir).find((f) => f.endsWith(".msi"));
results.package = {
  source: buildInfo.source,
  built_at: buildInfo.built_at,
  signing: buildInfo.signing,
  msi,
  msi_sha256: sha256(fs.readFileSync(path.join(packageDir, msi))),
};

const work = fs.mkdtempSync(path.join(os.tmpdir(), "downova-b33-"));
const ex = spawnSync("msiexec", [
  "/a",
  path.join(packageDir, msi),
  "/qn",
  `TARGETDIR=${path.join(work, "app")}`,
]);
if (ex.status !== 0) throw new Error(`msiexec /a exited ${ex.status}`);
function findFile(dir, name) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name.toLowerCase() === name) return p;
  }
  return null;
}
const serviceExe = findFile(path.join(work, "app"), "dm-service.exe");
const binDir = path.dirname(serviceExe);
const dmExe = path.join(binDir, "dm.exe");
const hostExe = findFile(path.join(work, "app"), "dm-browser-host.exe");
const home = path.join(work, "data");
const dl = path.join(work, "downloads");
fs.mkdirSync(dl, { recursive: true });
// A release build ignores test switches; make sure none leak in from the caller either.
const TEST_SWITCHES = new Set(["DOWNOVA_VAULT", "DOWNOVA_TEST_CA_DER"]);
const env = Object.fromEntries(
  Object.entries({ ...process.env, DOWNOVA_HOME: home, DOWNOVA_DOWNLOAD_DIR: dl }).filter(
    ([k]) => !TEST_SWITCHES.has(k),
  ),
);

function dm(args) {
  const r = spawnSync(dmExe, ["--data-dir", home, "--json", ...args], {
    env,
    encoding: "utf8",
    timeout: 60_000,
  });
  let json = null;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    // reported by the caller
  }
  return { code: r.status, json, raw: `${r.stdout}${r.stderr}` };
}
async function until(what, fn, ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const v = await fn();
    if (v) return v;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}
/** Credential Manager targets that mention Downova (names only; `cmdkey` never prints secrets). */
function downovaTargets() {
  const out = spawnSync("cmdkey", ["/list"], { encoding: "utf8" }).stdout ?? "";
  return out
    .split(/\r?\n/)
    .map((l) => /Target:\s*(.+)$/i.exec(l.trim())?.[1]?.trim())
    .filter((t) => t && /Downova\//.test(t));
}
function filesContaining(dir, secret) {
  const hits = [];
  const needle = Buffer.from(secret);
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else {
        try {
          if (fs.readFileSync(p).includes(needle)) hits.push(p);
        } catch {
          hits.push(`${p} (unreadable)`);
        }
      }
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return hits;
}
function frame(obj) {
  const body = Buffer.from(JSON.stringify(obj));
  const len = Buffer.alloc(4);
  len.writeUInt32LE(body.length);
  return Buffer.concat([len, body]);
}
function host(messages) {
  const r = spawnSync(hostExe, [ORIGIN_ARG], {
    env,
    input: Buffer.concat(messages.map(frame)),
    timeout: 60_000,
  });
  const replies = [];
  let rest = r.stdout ?? Buffer.alloc(0);
  while (rest.length >= 4) {
    const n = rest.readUInt32LE(0);
    replies.push(JSON.parse(rest.subarray(4, 4 + n).toString("utf8")));
    rest = rest.subarray(4 + n);
  }
  return { code: r.status, replies, stdout: r.stdout ?? Buffer.alloc(0), stderr: r.stderr ?? "" };
}

const before = new Set(downovaTargets());
const body = payload(2 * 1024 * 1024, 77);
const { server, base } = await startFixtureServer({ "pub.bin": body });
const service = spawn(serviceExe, ["--data-dir", home, "--download-dir", dl], {
  env,
  stdio: ["ignore", "ignore", "pipe"],
});
let serviceErr = "";
service.stderr.on("data", (d) => (serviceErr += d));
try {
  await until("the packaged service", () => dm(["hello"]).code === 0, 60_000);

  // Q1
  const added = dm(["add", `${base}/pub.bin?token=${SECRET}`]);
  const id = added.json?.data?.job_id;
  const done = await until(
    "the public download",
    () => {
      const j = dm(["get", id]).json?.data;
      return j && ["completed", "failed", "cancelled"].includes(j.phase) ? j : null;
    },
    120_000,
  );
  const file = path.join(dl, done.display_name);
  const created = downovaTargets().filter((t) => !before.has(t));
  const q1Hits = filesContaining(home, SECRET);
  record(
    "Q1 a protected public link completes; the data key is in Credential Manager; no data file holds the link's secret",
    done.phase === "completed" &&
      fs.existsSync(file) &&
      sha256(fs.readFileSync(file)) === sha256(body) &&
      created.length >= 1 &&
      q1Hits.length === 0
      ? "passed"
      : "failed",
    {
      phase: done.phase,
      source_display: done.source_display,
      credential_manager_targets_created: created,
      files_with_secret: q1Hits,
    },
  );

  // Q2
  const signedOrigin = "https://downova-b33.invalid";
  const signedUrl = `${signedOrigin}/reports/q3.pdf`;
  const signed = (rid, url, key, remember = true) => ({
    v: 1,
    request_id: rid,
    op: "add_authenticated",
    url,
    idempotency_key: key,
    session: {
      origin: signedOrigin,
      browser: "edge",
      profile_id: "0123456789abcdef",
      cookies: [
        {
          name: "sid",
          value: COOKIE,
          domain: "downova-b33.invalid",
          host_only: true,
          path: "/",
          secure: true,
        },
      ],
      remember,
    },
  });
  const h = host([signed("req_pkg00001", signedUrl, "ext-pkg-key-0001")]);
  const reply = h.replies[0];
  const job = reply?.job;
  const creds = dm(["credentials"]);
  const list = creds.json?.data?.credentials ?? [];
  const afterQ2 = downovaTargets().filter((t) => !before.has(t));
  const leaks = [
    h.stdout.includes(Buffer.from(COOKIE)) && "host stdout",
    String(h.stderr).includes(COOKIE) && "host stderr",
    creds.raw.includes(COOKIE) && "dm credentials",
    dm(["get", job?.job_id ?? "job_0000000000000000"]).raw.includes(COOKIE) && "dm get",
    ...filesContaining(home, COOKIE),
  ].filter(Boolean);
  record(
    "Q2 a remembered signed-in hand-off through the packaged host: accepted, metadata only, a Credential Manager entry, no cookie anywhere",
    reply?.outcome === "accepted" &&
      job?.auth?.mode === "remembered" &&
      job?.auth?.origin === signedOrigin &&
      list.length === 1 &&
      list[0].mode === "remembered" &&
      afterQ2.length > created.length &&
      leaks.length === 0
      ? "passed"
      : "failed",
    {
      outcome: reply?.outcome,
      auth: job?.auth,
      credentials: list,
      credential_manager_targets_created: afterQ2,
      leaks,
    },
  );

  // Q3
  const refused = host([
    signed("req_pkg00002", "https://other.invalid/reports/q3.pdf", "ext-pkg-key-0002", false),
    signed("req_pkg00003", "http://downova-b33.invalid/reports/q3.pdf", "ext-pkg-key-0003", false),
  ]);
  const jobsNow = dm(["list"]).json?.data?.items ?? [];
  record(
    "Q3 another origin and http are refused by the host; no job is created",
    refused.replies.length === 2 &&
      refused.replies.every(
        (r) => r.outcome === "rejected" && r.error?.kind === "AUTH_SCOPE_UNSUPPORTED",
      ) &&
      jobsNow.length === 2
      ? "passed"
      : "failed",
    { replies: refused.replies.map((r) => [r.outcome, r.error?.kind]), jobs: jobsNow.length },
  );

  // Q4
  const credId = list[0]?.credential_id ?? "cred_0000000000000000";
  const forgot = dm(["credentials-forget", credId]);
  const afterForget = downovaTargets().filter((t) => !before.has(t));
  const jobAfter = dm(["get", job?.job_id ?? "job_0000000000000000"]).json?.data;
  record(
    "Q4 forget removes the sign-in from the service and Credential Manager and pauses its download",
    forgot.code === 0 &&
      (forgot.json?.data?.credentials ?? []).length === 0 &&
      afterForget.length === afterQ2.length - 1 &&
      jobAfter?.desired_state === "paused"
      ? "passed"
      : "failed",
    {
      code: forgot.code,
      remaining: forgot.json?.data?.credentials,
      credential_manager_targets: afterForget,
      job: jobAfter && { desired_state: jobAfter.desired_state, auth: jobAfter.auth },
    },
  );
  // Q5 (ADR 0028): the forgotten download waits for a reconnect; the packaged host lists it for
  // this origin and profile only, and a reconnect with a fresh sign-in is accepted on that job.
  const FRESH = "SYNTH-PKG-FRESH-b33";
  const listed = host([
    {
      v: 1,
      request_id: "req_pkg00010",
      op: "reconnect_candidates",
      origin: signedOrigin,
      browser: "edge",
      profile_id: "0123456789abcdef",
    },
    {
      v: 1,
      request_id: "req_pkg00011",
      op: "reconnect_candidates",
      origin: signedOrigin,
      browser: "edge",
      profile_id: "fedcba9876543210",
    },
  ]);
  const offered = (listed.replies[0]?.candidates ?? []).map((c) => c.job_id);
  const fresh = signed("req_pkg00012", signedUrl, "unused-key-0012", false).session;
  fresh.cookies[0].value = FRESH;
  const rc = host([
    {
      v: 1,
      request_id: "req_pkg00012",
      op: "reconnect",
      job_id: job?.job_id,
      operation_id: "ext-pkg-reconnect-01",
      expected_credential_id: listed.replies[0]?.candidates?.[0]?.auth?.credential_id,
      session: fresh,
      resume: false,
    },
  ]);
  const after = rc.replies[0]?.job;
  const q5Leaks = [
    rc.stdout.includes(Buffer.from(FRESH)) && "host stdout",
    String(rc.stderr).includes(FRESH) && "host stderr",
    ...filesContaining(home, FRESH),
  ].filter(Boolean);
  record(
    "Q5 after Forget the download waits; the packaged host lists it for this origin and profile only; a reconnect with a fresh sign-in is accepted on that job and keeps its pause",
    offered.length === 1 &&
      offered[0] === job?.job_id &&
      (listed.replies[1]?.candidates ?? []).length === 0 &&
      rc.replies[0]?.outcome === "accepted" &&
      after?.job_id === job?.job_id &&
      after?.auth?.state === "available" &&
      after?.desired_state === "paused" &&
      !(after?.blockers ?? []).includes("sign_in_required") &&
      q5Leaks.length === 0
      ? "passed"
      : "failed",
    {
      offered,
      other_profile: (listed.replies[1]?.candidates ?? []).length,
      outcome: rc.replies[0]?.outcome,
      error: rc.replies[0]?.error?.kind,
      after: after && {
        auth: after.auth,
        desired_state: after.desired_state,
        blockers: after.blockers,
      },
      leaks: q5Leaks,
    },
  );
  record(
    "the release service's stderr never held a secret",
    serviceErr.includes(SECRET) || serviceErr.includes(COOKIE) ? "failed" : "passed",
  );
  dm(["shutdown"]);
  await until("the service to stop", () => service.exitCode !== null, 30_000).catch(() => {
    service.kill();
  });
} catch (e) {
  record("run", "failed", { error: String(e?.message ?? e) });
  service.kill();
} finally {
  server.close();
  // Remove exactly the Credential Manager entries this run created (the data key, sign-ins).
  const mine = downovaTargets().filter((t) => !before.has(t));
  for (const t of mine) spawnSync("cmdkey", [`/delete:${t}`]);
  results.credential_manager_cleanup = {
    deleted: mine,
    left: downovaTargets().filter((t) => !before.has(t)),
  };
}
finish();
