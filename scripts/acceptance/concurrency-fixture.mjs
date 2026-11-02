#!/usr/bin/env node
// Fixture process for scripts/acceptance/concurrency-acceptance.mjs (CORE-02, E03).
//
//   node scripts/acceptance/concurrency-fixture.mjs <size-bytes> <name>:<seed> [...]
//
// Serves the named deterministic payloads (range-capable, strong ETag, `?slow=<ms>` per 64 KiB)
// and prints its base URL on the first stdout line. It runs in its own process so the acceptance
// script's blocking CLI calls never delay its socket bookkeeping. `GET /__stats` answers:
// - served: requests and body bytes per file;
// - overlap: the most requests open at once, not counting one the server saw close within
//   TEARDOWN_MS after another opened (a client that drops a response closes its socket at once;
//   the server notices at its next write) — the same rule as the Rust fixture's max_overlap;
// - raw_peak: the plain count including those not-yet-noticed closes.
import { payload, startFixtureServer } from "../lib/fixture.mjs";

const TEARDOWN_MS = 150;
const [size, ...specs] = process.argv.slice(2);
const bodies = Object.fromEntries(
  specs.map((s) => {
    const [name, seed] = s.split(":");
    return [name, payload(Number(size), Number(seed))];
  }),
);
const served = Object.fromEntries(Object.keys(bodies).map((n) => [n, { requests: 0, bytes: 0 }]));
const lifetimes = [];
let open = 0;
let rawPeak = 0;
const overlap = () => {
  let best = 0;
  for (const a of lifetimes) {
    const n = lifetimes.filter(
      (b) => b.start <= a.start && (b.end === null || b.end > a.start + TEARDOWN_MS),
    ).length;
    best = Math.max(best, n);
  }
  return best;
};
const { server, base } = await startFixtureServer(bodies, {
  extra: (req, res, url) => {
    if (url.pathname !== "/__stats") return false;
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        served,
        overlap: overlap(),
        raw_peak: rawPeak,
        // Per request: file, range header, start and end (ms since the fixture started), bytes.
        log: lifetimes.map((l) => ({
          ...l,
          start: Math.round(l.start),
          end: l.end && Math.round(l.end),
        })),
      }),
    );
    return true;
  },
});
server.on("request", (req, res) => {
  const name = new URL(req.url, "http://x").pathname.slice(1);
  if (!served[name]) return;
  served[name].requests += 1;
  open += 1;
  rawPeak = Math.max(rawPeak, open);
  const life = {
    name,
    range: req.headers.range ?? null,
    start: performance.now(),
    end: null,
    bytes: 0,
  };
  lifetimes.push(life);
  const write = res.write.bind(res);
  res.write = (chunk, ...rest) => {
    served[name].bytes += chunk.length;
    life.bytes += chunk.length;
    return write(chunk, ...rest);
  };
  res.on("close", () => {
    life.end = performance.now();
    open -= 1;
  });
});
process.stdout.write(`${base}\n`);
