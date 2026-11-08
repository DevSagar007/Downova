import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  DownloadError,
  backoffMs,
  downloadVerified,
  isTransientToolDownloadFailure,
  retryAfterMs,
  sanitizeUrl,
} from "./download.mjs";
import { installedMatches } from "../fetch-aria2.mjs";

const BODY = Buffer.from("pinned tool bytes");
const SHA = createHash("sha256").update(BODY).digest("hex");
const quiet = { log: () => {}, sleep: async () => {} };
const response = (status, body = "", headers = {}) =>
  new Response(status === 204 ? null : body, { status, headers });

test("a transient 504 is retried and the verified bytes are returned (real HTTP)", async () => {
  let hits = 0;
  const server = http.createServer((req, res) => {
    hits += 1;
    if (hits === 1) res.writeHead(504).end("gateway timeout");
    else res.writeHead(200, { "Content-Length": BODY.length }).end(BODY);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const logs = [];
  try {
    const bytes = await downloadVerified(`http://127.0.0.1:${server.address().port}/t.zip`, {
      tool: "fixture",
      size: BODY.length,
      sha256: SHA,
      log: (l) => logs.push(l),
      sleep: async () => {},
    });
    assert.deepEqual(bytes, BODY);
    assert.equal(hits, 2);
    assert.ok(
      logs.some((l) => /HTTP 504/.test(l) && /attempt 1\/5/.test(l)),
      logs.join("\n"),
    );
  } finally {
    server.close();
  }
});

test("repeated timeouts stop at the finite attempt limit", async () => {
  let calls = 0;
  const fetchImpl = async () => {
    calls += 1;
    throw Object.assign(new Error("timeout"), { name: "TimeoutError" });
  };
  await assert.rejects(
    downloadVerified("https://example.invalid/t.zip", {
      tool: "t",
      sha256: SHA,
      attempts: 3,
      fetchImpl,
      ...quiet,
    }),
    (e) => e instanceof DownloadError && /timed out/.test(e.message) && /gave up/.test(e.message),
  );
  assert.equal(calls, 3);
});

test("a permanent HTTP error fails at once, without retries", async () => {
  let calls = 0;
  const fetchImpl = async () => {
    calls += 1;
    return response(404, "missing");
  };
  await assert.rejects(
    downloadVerified("https://example.invalid/t.zip", {
      tool: "t",
      sha256: SHA,
      fetchImpl,
      ...quiet,
    }),
    (e) => !e.transient && /HTTP 404/.test(e.message),
  );
  assert.equal(calls, 1);
});

test("a checksum or size mismatch is never retried and returns nothing", async () => {
  let calls = 0;
  const fetchImpl = async () => {
    calls += 1;
    return response(200, "something else");
  };
  await assert.rejects(
    downloadVerified("https://example.invalid/t.zip", {
      tool: "t",
      sha256: SHA,
      fetchImpl,
      ...quiet,
    }),
    (e) => !e.transient && /checksum mismatch/.test(e.message),
  );
  assert.equal(calls, 1);
});

test("Retry-After is honoured only within the total budget", async () => {
  let calls = 0;
  const waits = [];
  const fetchImpl = async () => {
    calls += 1;
    return response(503, "", { "Retry-After": "120" });
  };
  await assert.rejects(
    downloadVerified("https://example.invalid/t.zip", {
      tool: "t",
      sha256: SHA,
      budgetMs: 60_000,
      fetchImpl,
      log: () => {},
      sleep: async (ms) => waits.push(ms),
    }),
    /gave up/,
  );
  assert.equal(calls, 1, "a 120 s Retry-After does not fit a 60 s budget");
  assert.deepEqual(waits, []);
  assert.equal(retryAfterMs("7"), 7000);
  assert.equal(retryAfterMs("not a date"), null);
  assert.equal(backoffMs(1, null), 5000);
  assert.equal(backoffMs(9, null), 60_000);
});

test("logged URLs carry no query string or credentials", () => {
  assert.equal(
    sanitizeUrl("https://user:secret@objects.example/a/b.zip?X-Amz-Signature=abc&token=t"),
    "https://objects.example/a/b.zip",
  );
});

test("only a positively identified transient tool download allows a packaging retry", () => {
  const nsis504 =
    "Downloading https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip\nfailed to bundle project: http status: 504";
  assert.equal(isTransientToolDownloadFailure(nsis504), true);
  assert.equal(
    isTransientToolDownloadFailure(
      "Downloading https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-x86_64.AppImage\ntimeout: global",
    ),
    true,
  );
  // Never retried: a hash mismatch, a compile error, or a 5xx without a known tool.
  assert.equal(
    isTransientToolDownloadFailure(`${nsis504}\nhash mismatch of downloaded file`),
    false,
  );
  assert.equal(
    isTransientToolDownloadFailure(
      "error[E0425]: cannot find value\ncould not compile `dm-desktop`",
    ),
    false,
  );
  assert.equal(isTransientToolDownloadFailure("failed: http status: 504"), false);
  assert.equal(isTransientToolDownloadFailure("nsis-3.13.zip\nhttp status: 404"), false);
});

test("a partial or corrupt aria2 install is rejected; a complete one is reused", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-aria2-"));
  try {
    const files = { "aria2c.exe": "exe", COPYING: "c", "LICENSE.OpenSSL": "l", AUTHORS: "a" };
    for (const [f, b] of Object.entries(files)) fs.writeFileSync(path.join(dir, f), b);
    const sha = (b) => createHash("sha256").update(b).digest("hex");
    assert.equal(installedMatches(dir), false, "no marker: never trusted because the files exist");
    const marker = {
      zip_sha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
      files: Object.fromEntries(Object.entries(files).map(([f, b]) => [f, sha(b)])),
    };
    fs.writeFileSync(path.join(dir, ".pin.json"), JSON.stringify(marker));
    assert.equal(installedMatches(dir), true);
    fs.writeFileSync(path.join(dir, "aria2c.exe"), "ex"); // truncated or changed
    assert.equal(installedMatches(dir), false);
    fs.writeFileSync(path.join(dir, "aria2c.exe"), "exe");
    fs.writeFileSync(
      path.join(dir, ".pin.json"),
      JSON.stringify({ ...marker, zip_sha256: "0".repeat(64) }),
    );
    assert.equal(installedMatches(dir), false, "another pin");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a restored tool cache keeps verified entries and drops only the changed one", async () => {
  const { snapshot, validate } = await import("./tool-cache.mjs");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "downova-toolcache-"));
  try {
    for (const [d, f, b] of [
      ["NSIS", "makensis.exe", "n"],
      ["WixTools314", "candle.exe", "w"],
    ]) {
      fs.mkdirSync(path.join(dir, d), { recursive: true });
      fs.writeFileSync(path.join(dir, d, f), b);
    }
    snapshot(dir);
    assert.deepEqual(validate(dir), { kept: ["NSIS", "WixTools314"], removed: [] });
    fs.writeFileSync(path.join(dir, "WixTools314", "candle.exe"), "w!"); // corrupted
    fs.writeFileSync(path.join(dir, "NSIS", "extra.dll"), "x"); // unrecorded file
    const r = validate(dir);
    assert.deepEqual(r.removed.sort(), ["NSIS", "WixTools314"]);
    assert.ok(!fs.existsSync(path.join(dir, "WixTools314")));
    fs.mkdirSync(path.join(dir, "Other"));
    fs.rmSync(path.join(dir, ".downova-tools.json"));
    assert.deepEqual(validate(dir).removed, ["Other"], "without a manifest nothing is trusted");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
