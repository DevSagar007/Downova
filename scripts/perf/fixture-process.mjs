#!/usr/bin/env node
// The measurement fixture in its own process (scripts/perf/measure.mjs): its CPU and memory are
// never counted as Downova's.
//
//   node scripts/perf/fixture-process.mjs [--count N] [--size-mib M]
//
// Serves `f<N>.bin` (default 4 files of 64 MiB; ranges and a strong ETag; `?slow=ms` per 64 KiB
// chunk). Each file has its own deterministic payload. The first stdout line is JSON:
// `{ "base": "http://127.0.0.1:<port>", "sha256": { "f0.bin": "…", … } }`, so the controller can
// check final integrity without holding the payloads itself (B2.32).
import { payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

const argv = process.argv.slice(2);
const arg = (n, d) => {
  const i = argv.indexOf(n);
  return i >= 0 ? Number(argv[i + 1]) : d;
};
const count = arg("--count", 4);
const size = arg("--size-mib", 64) * 1024 * 1024;
const bodies = {};
const hashes = {};
for (let n = 0; n < count; n++) {
  const body = payload(size, 4242 + n);
  bodies[`f${n}.bin`] = body;
  hashes[`f${n}.bin`] = sha256(body);
}
const { base } = await startFixtureServer(bodies);
process.stdout.write(`${JSON.stringify({ base, sha256: hashes })}\n`);
