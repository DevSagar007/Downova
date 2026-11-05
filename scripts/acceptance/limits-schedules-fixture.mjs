#!/usr/bin/env node
// Fixture process for the B3.1 acceptance and throughput scripts (CORE-06, CORE-08).
//
//   node scripts/acceptance/limits-schedules-fixture.mjs <name>:<size-bytes>:<seed> [...]
//
// Serves deterministic payloads (scripts/lib/fixture.mjs `payload`) at full speed with a strong
// ETag and Range/If-Range support; `?norange=1` serves a server without range support (always 200
// with the whole body, no `Accept-Ranges`). Writes honour socket back-pressure (each 16 KiB write
// waits for `drain` when the socket buffer is full), so the per-file `bytes_written` counter
// follows what the client actually accepts, give or take the socket buffers: that is what makes
// it usable as a server-side throughput observer of a throttled client. (The shared
// `startFixtureServer` writes without waiting, which hands a whole body to the socket at once.)
//
// Runs in its own process, so the caller's blocking CLI calls never delay its writes or counters.
// Prints `{ "base": …, "sha256": { name: hash } }` as its first stdout line. `GET /__stats`:
// per-file requests, full and range responses, range starts, bytes written, completed and aborted
// responses, and `now_ms` (since the fixture started).
import http from "node:http";
import { payload, sha256 } from "../lib/fixture.mjs";

const CHUNK = 16 * 1024;
const specs = process.argv.slice(2).map((s) => {
  const [name, size, seed] = s.split(":");
  return { name, size: Number(size), seed: Number(seed) };
});
const bodies = Object.fromEntries(specs.map((s) => [s.name, payload(s.size, s.seed)]));
const etags = Object.fromEntries(
  Object.entries(bodies).map(([n, b]) => [n, `"${sha256(b).slice(0, 16)}"`]),
);
const t0 = performance.now();
const files = {};
const fileStats = (name) =>
  (files[name] ??= {
    requests: 0,
    full_responses: 0,
    range_responses: 0,
    range_starts: [],
    bytes_written: 0,
    completed_responses: 0,
    aborted_responses: 0,
  });

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://x");
  if (url.pathname === "/__stats") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ files, now_ms: Math.round(performance.now() - t0) }));
    return;
  }
  const name = url.pathname.slice(1);
  const body = bodies[name];
  if (!body) return res.writeHead(404).end();
  const rec = fileStats(name);
  rec.requests += 1;
  const noRange = url.searchParams.get("norange") === "1";
  const range = noRange ? null : /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
  const ifRange = req.headers["if-range"];
  let start = 0;
  let end = body.length - 1;
  let status = 200;
  if (range && (!ifRange || ifRange === etags[name])) {
    start = Number(range[1]);
    end = range[2] ? Math.min(Number(range[2]), body.length - 1) : body.length - 1;
    status = 206;
  }
  const headers = { "Content-Length": end - start + 1 };
  if (!noRange) {
    headers.ETag = etags[name];
    headers["Accept-Ranges"] = "bytes";
  }
  if (status === 206) {
    headers["Content-Range"] = `bytes ${start}-${end}/${body.length}`;
    rec.range_responses += 1;
    if (rec.range_starts.length < 200) rec.range_starts.push(start);
  } else rec.full_responses += 1;
  res.writeHead(status, headers);
  let closed = false;
  res.on("close", () => (closed = true));
  for (let i = start; i <= end; i += CHUNK) {
    if (closed || res.destroyed) {
      rec.aborted_responses += 1;
      return;
    }
    const part = body.subarray(i, Math.min(i + CHUNK, end + 1));
    const flushed = res.write(part);
    rec.bytes_written += part.length;
    if (!flushed) {
      await new Promise((r) => {
        const done = () => {
          res.off("drain", done);
          res.off("close", done);
          r();
        };
        res.on("drain", done);
        res.on("close", done);
      });
    }
  }
  res.end();
  rec.completed_responses += 1;
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;
const hashes = Object.fromEntries(Object.entries(bodies).map(([n, b]) => [n, sha256(b)]));
process.stdout.write(`${JSON.stringify({ base, sha256: hashes })}\n`);
