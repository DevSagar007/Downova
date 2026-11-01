#!/usr/bin/env node
// The measurement fixture in its own process (scripts/perf/measure.mjs): its CPU and memory are
// never counted as Downova's. Prints its base URL on the first stdout line, then serves
// `f<N>.bin` (64 MiB each, ranges and a strong ETag; `?slow=ms` per 64 KiB chunk).
import { payload, startFixtureServer } from "../lib/fixture.mjs";

const body = payload(64 * 1024 * 1024, 4242);
const bodies = Object.fromEntries([0, 1, 2, 3].map((n) => [`f${n}.bin`, body]));
const { base } = await startFixtureServer(bodies);
process.stdout.write(`${base}\n`);
