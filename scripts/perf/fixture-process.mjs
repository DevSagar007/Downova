#!/usr/bin/env node
// The measurement fixture in its own process (scripts/perf/measure.mjs): its CPU and memory are
// never counted as Downova's.
//
//   node scripts/perf/fixture-process.mjs [--count N] [--size-mib M]
//        [--host ADDR] [--port P] [--public-host NAME]
//
// Serves `f<N>.bin` (default 4 files of 64 MiB; ranges and a strong ETag; `?slow=ms` per 64 KiB
// chunk; `?norange=1` for a server without range support). Each file has its own deterministic
// payload. The first stdout line is JSON:
// `{ "base": "http://<public-host>:<port>", "sha256": { "f0.bin": "…", … } }`, so the controller
// can check final integrity without holding the payloads itself (B2.32).
//
// B2.33 (the IDM comparison protocol, docs/benchmarks/idm-comparison-protocol.md):
//   --host         the address to bind (default 127.0.0.1, local only). On a second machine use
//                  that machine's LAN address, or 0.0.0.0 together with --public-host.
//   --port         a fixed port (default: any free port), so the client's firewall rule and the
//                  URLs stay the same across runs.
//   --public-host  the host name or address written into `base` (default: --host). Required with
//                  a wildcard bind (0.0.0.0 or ::), which no client can connect to as such.
//   GET /__stats   per-file statistics since start: requests, full (200) and ranged (206)
//                  responses, the first range start offsets, bytes handed to the socket, completed
//                  and aborted responses (scripts/lib/fixture.mjs `createStats`). This is the
//                  server-side evidence for the resume workload (W5): a resume shows a ranged
//                  request starting past 0 and total bytes close to the file size.
//   GET /__stats?reset=1  returns the statistics, then starts counting again (one run each).
import { createStats, payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

const argv = process.argv.slice(2);
const str = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? argv[i + 1] : d;
};
const num = (n, d) => Number(str(n, d));
const count = num("--count", 4);
const size = num("--size-mib", 64) * 1024 * 1024;
const host = str("--host", "127.0.0.1");
const port = num("--port", 0);
const wildcard = host === "0.0.0.0" || host === "::";
const publicHost = str("--public-host", wildcard ? undefined : host);
if (!publicHost) {
  process.stderr.write("--host 0.0.0.0 or :: needs --public-host <the address clients use>\n");
  process.exit(2);
}
if (!Number.isInteger(count) || count < 1 || !(size > 0) || !Number.isInteger(port)) {
  process.stderr.write("--count, --size-mib and --port must be positive numbers\n");
  process.exit(2);
}

const bodies = {};
const hashes = {};
for (let n = 0; n < count; n++) {
  const body = payload(size, 4242 + n);
  bodies[`f${n}.bin`] = body;
  hashes[`f${n}.bin`] = sha256(body);
}
const stats = createStats();
const extra = async (req, res, url) => {
  if (url.pathname !== "/__stats") return false;
  const body = JSON.stringify({ ...stats, at: new Date().toISOString() });
  // Reset in place: the server holds this one collector.
  if (url.searchParams.get("reset") === "1") Object.assign(stats, createStats());
  res.writeHead(200, { "Content-Type": "application/json" }).end(body);
  return true;
};
const { server } = await startFixtureServer(bodies, { host, port, extra, stats });
const base = `http://${publicHost.includes(":") ? `[${publicHost}]` : publicHost}:${server.address().port}`;
process.stdout.write(`${JSON.stringify({ base, sha256: hashes })}\n`);
