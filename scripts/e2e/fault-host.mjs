#!/usr/bin/env node
// Fault-injecting native-messaging host for the real-browser test (B2.33). TEST ONLY.
//
//   <wrapper> → node scripts/e2e/fault-host.mjs <config.json> <arguments the browser passed>
//
// scripts/e2e/chrome-handoff.mjs (`--fault-steps`) points the browser's Downova manifest — in the
// run's isolated profile or the packaged smoke's isolated LOCALAPPDATA/HOME, never a real user's —
// at a small wrapper that starts this script, and restores the manifest afterwards. This script
// reads one framed request from the browser and, depending on `mode` in the config file (read on
// every launch, so the harness switches modes between steps without re-registering):
//
//   pass               forwards to the real packaged host and relays its reply unchanged;
//   silent             never answers (the helper hangs); exits when the browser closes stdin or
//                      after `silent_ms` (default 120 s);
//   exit-after-accept  forwards to the real host (the service accepts and creates the job), then
//                      exits without replying: the acknowledgement is lost after acceptance;
//   mismatch           forwards, then replies with the real answer under a different request id:
//                      an acknowledgement that does not match the request;
//   unavailable        runs the real host with DOWNOVA_HOME pointing at an empty folder, so it
//                      finds no service and answers `unavailable` (the host never starts one).
//
// Every launch appends one JSON line to `log`: mode, request (op, request id, idempotency key,
// URL path only), the real host's outcome and job id, and what was sent back. Nothing here is part
// of the product; the product's host is crates/browser-host.
import { spawn } from "node:child_process";
import fs from "node:fs";
import { createDecoder, encode } from "../lib/native-messaging.mjs";

const [configPath, ...hostArgs] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
const mode = config.mode;
const entry = { t: new Date().toISOString(), mode, host_args: hostArgs.length };
const writeLog = () => {
  try {
    fs.appendFileSync(config.log, `${JSON.stringify(entry)}\n`);
  } catch {
    // a missing log never changes the outcome under test
  }
};

/** The first framed message on `stream`, or null when it ends first. */
function readOne(stream, timeoutMs) {
  return new Promise((resolve) => {
    const d = createDecoder();
    const timer = setTimeout(() => finish(null), timeoutMs);
    const finish = (v) => {
      clearTimeout(timer);
      stream.off("data", onData);
      stream.off("end", onEnd);
      resolve(v);
    };
    const onData = (chunk) => {
      const [m] = d.push(chunk);
      if (m !== undefined) finish(m);
    };
    const onEnd = () => finish(null);
    stream.on("data", onData);
    stream.once("end", onEnd);
  });
}

/** One request through the real host; resolves to its framed reply (or null). */
async function viaRealHost(request, env) {
  const child = spawn(config.real_host, hostArgs, { env, stdio: ["pipe", "pipe", "ignore"] });
  child.stdin.end(encode(request));
  const reply = await readOne(child.stdout, 30_000);
  child.kill();
  return reply;
}

const request = await readOne(process.stdin, 30_000);
let pathOnly = null;
try {
  pathOnly = request?.url ? new URL(request.url).pathname : null;
} catch {
  pathOnly = "(invalid)";
}
entry.request = request && {
  op: request.op ?? null,
  request_id: request.request_id ?? null,
  idempotency_key: request.idempotency_key ?? null,
  url_path: pathOnly,
};
const summarize = (r) =>
  r && {
    outcome: r.outcome ?? null,
    job_id: r.job?.job_id ?? null,
    request_id: r.request_id ?? null,
  };

if (!request) {
  entry.sent = "nothing (no request)";
  writeLog();
  process.exit(0);
}

switch (mode) {
  case "silent": {
    entry.sent = "nothing (silent)";
    writeLog();
    // Stay alive without answering until the browser gives up on us.
    const until = Date.now() + (config.silent_ms ?? 120_000);
    process.stdin.resume();
    process.stdin.once("end", () => process.exit(0));
    while (Date.now() < until) await new Promise((r) => setTimeout(r, 500));
    process.exit(0);
    break;
  }
  case "exit-after-accept": {
    const reply = await viaRealHost(request, process.env);
    entry.real_reply = summarize(reply);
    entry.sent = "nothing (exited after the real host answered)";
    writeLog();
    process.exit(0);
    break;
  }
  case "mismatch": {
    const reply = await viaRealHost(request, process.env);
    entry.real_reply = summarize(reply);
    const forged = { ...reply, request_id: `mismatch${String(request.request_id).slice(0, 40)}` };
    entry.sent = { mismatched_request_id: forged.request_id };
    writeLog();
    process.stdout.write(encode(forged), () => process.exit(0));
    break;
  }
  case "unavailable": {
    fs.mkdirSync(config.empty_home, { recursive: true });
    const reply = await viaRealHost(request, { ...process.env, DOWNOVA_HOME: config.empty_home });
    entry.real_reply = summarize(reply);
    entry.sent = "real reply (no service for the empty data folder)";
    writeLog();
    process.stdout.write(encode(reply), () => process.exit(0));
    break;
  }
  default: {
    // pass
    const reply = await viaRealHost(request, process.env);
    entry.real_reply = summarize(reply);
    entry.sent = "real reply";
    writeLog();
    if (reply) process.stdout.write(encode(reply), () => process.exit(0));
    else process.exit(0);
  }
}
