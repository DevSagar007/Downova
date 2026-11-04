// node --test scripts/lib/fixture.test.mjs (part of `pnpm verify`, group structure).
import { test } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import { createStats, payload, sha256, startFixtureServer } from "./fixture.mjs";
import { readFirstJsonLine } from "./first-line.mjs";

const get = (url, headers = {}) =>
  new Promise((resolve, reject) => {
    http
      .get(url, { headers }, (res) => {
        const parts = [];
        res.on("data", (d) => parts.push(d));
        res.on("end", () =>
          resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(parts) }),
        );
      })
      .on("error", reject);
  });

test("statistics count full, ranged and aborted responses per file", async () => {
  const body = payload(256 * 1024, 9);
  const stats = createStats();
  const { server, base } = await startFixtureServer({ "a.bin": body }, { stats, chunk: 16384 });
  try {
    const full = await get(`${base}/a.bin`);
    assert.equal(full.status, 200);
    assert.equal(sha256(full.body), sha256(body));
    const ranged = await get(`${base}/a.bin`, { Range: "bytes=200000-" });
    assert.equal(ranged.status, 206);
    assert.equal(ranged.body.length, body.length - 200000);
    // A client that goes away early: the response is counted as aborted.
    await new Promise((resolve) => {
      const req = http.get(`${base}/a.bin?slow=50`, (res) => {
        res.once("data", () => {
          req.destroy();
          setTimeout(resolve, 300);
        });
      });
      req.on("error", () => undefined);
    });
    const f = stats.files["a.bin"];
    assert.equal(f.requests, 3);
    assert.equal(f.full_responses, 2);
    assert.equal(f.range_responses, 1);
    assert.deepEqual(f.range_starts, [200000]);
    assert.equal(f.completed_responses, 2);
    assert.equal(f.aborted_responses, 1);
    // Both complete responses fully, plus at least the first chunk of the aborted one.
    assert.ok(f.bytes_written >= body.length + (body.length - 200000) + 16384);
    assert.equal(stats.total_bytes_written, f.bytes_written);
    assert.equal(stats.open_responses, 0);
    assert.equal(stats.max_open_responses, 1);
  } finally {
    server.close();
  }
});

test("the peak of concurrently open responses is the server's own view", async () => {
  const stats = createStats();
  const { server, base } = await startFixtureServer(
    { "a.bin": payload(128 * 1024, 1), "b.bin": payload(128 * 1024, 2) },
    { stats, chunk: 16384 },
  );
  try {
    await Promise.all([get(`${base}/a.bin?slow=20`), get(`${base}/b.bin?slow=20`)]);
    assert.equal(stats.max_open_responses, 2);
    // The client has every byte before the server ends its last `slow` pause and the response.
    for (let i = 0; i < 50 && stats.open_responses > 0; i++)
      await new Promise((r) => setTimeout(r, 20));
    assert.equal(stats.open_responses, 0);
  } finally {
    server.close();
  }
});

test("?norange=1 ignores Range and advertises none", async () => {
  const body = payload(100000, 3);
  const { server, base } = await startFixtureServer({ "a.bin": body });
  try {
    const r = await get(`${base}/a.bin?norange=1`, { Range: "bytes=10-" });
    assert.equal(r.status, 200);
    assert.equal(r.headers["accept-ranges"], undefined);
    assert.equal(r.body.length, 100000);
  } finally {
    server.close();
  }
});

test("without stats the fixture behaves as before", async () => {
  const { server, base } = await startFixtureServer({ "a.bin": payload(1000, 1) });
  try {
    assert.match(base, /^http:\/\/127\.0\.0\.1:\d+$/);
    assert.equal((await get(`${base}/a.bin`)).status, 200);
  } finally {
    server.close();
  }
});

const script = path.join(import.meta.dirname, "..", "perf", "fixture-process.mjs");

test("fixture-process refuses a wildcard bind without a public host", () => {
  const r = spawnSync(
    process.execPath,
    [script, "--count", "1", "--size-mib", "1", "--host", "0.0.0.0"],
    {
      encoding: "utf8",
    },
  );
  assert.equal(r.status, 2);
  assert.match(r.stderr, /--public-host/);
});

test("fixture-process: public host in base, /__stats with reset", async () => {
  const child = spawn(
    process.execPath,
    [script, "--count", "2", "--size-mib", "1", "--host", "0.0.0.0", "--public-host", "127.0.0.1"],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  try {
    const first = await readFirstJsonLine(child, { timeoutMs: 30_000 });
    assert.match(first.base, /^http:\/\/127\.0\.0\.1:\d+$/);
    assert.deepEqual(Object.keys(first.sha256), ["f0.bin", "f1.bin"]);
    const f1 = await get(`${first.base}/f1.bin`, { Range: "bytes=524288-" });
    assert.equal(f1.status, 206);
    const s = JSON.parse((await get(`${first.base}/__stats?reset=1`)).body);
    assert.equal(s.files["f1.bin"].range_responses, 1);
    assert.deepEqual(s.files["f1.bin"].range_starts, [524288]);
    assert.equal(s.files["f1.bin"].bytes_written, 524288);
    const after = JSON.parse((await get(`${first.base}/__stats`)).body);
    assert.deepEqual(after.files, {});
    assert.equal(after.total_bytes_written, 0);
  } finally {
    child.kill();
  }
});
