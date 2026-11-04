// Controlled fixture server shared by the packaged smoke, the real-browser test and the manual
// acceptance procedure (B2.6).
//
// Deterministic payloads (xorshift32 from a seed) with a strong ETag, Content-Length and Range
// support (`If-Range` honoured; `?norange=1` turns it off); `?slow=<ms>` delays every 64 KiB so a transfer is still running
// when a window or browser is closed.
import { createHash } from "node:crypto";
import http from "node:http";

export const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

export function payload(len, seed) {
  const out = Buffer.alloc(len);
  let s = seed >>> 0;
  for (let i = 0; i < len; i++) {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    out[i] = s & 0xff;
  }
  return out;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A per-file statistics collector for `startFixtureServer({ stats })` (B2.33). */
export function createStats() {
  // `open_responses`/`max_open_responses`: body responses in progress across all files, as the
  // server sees them (a response counts until the server finishes or notices the client left).
  return {
    started_at: new Date().toISOString(),
    total_bytes_written: 0,
    open_responses: 0,
    max_open_responses: 0,
    files: {},
  };
}
/** The first 100 range start offsets are kept: enough to show where each resume began. */
const MAX_STARTS = 100;
function fileStats(stats, name, status, start) {
  const rec = (stats.files[name] ??= {
    requests: 0,
    full_responses: 0,
    range_responses: 0,
    range_starts: [],
    bytes_written: 0,
    completed_responses: 0,
    aborted_responses: 0,
  });
  rec.requests += 1;
  if (status === 206) {
    rec.range_responses += 1;
    if (rec.range_starts.length < MAX_STARTS) rec.range_starts.push(start);
  } else rec.full_responses += 1;
  return rec;
}

/**
 * Serves `bodies` (name → Buffer) on `host:port` (0 = any free port). `extra(req, res, url)` may
 * answer other paths first and returns true when it did. `chunk` is the bytes written per step (and
 * per `?slow` pause): smaller steps notice a client that closed sooner, since a closed socket is
 * only seen at the next write. Resolves to `{ server, base }`.
 *
 * `stats` (optional, B2.33): an object that `createStats()` returned; every body response is
 * counted in it per file (requests, ranged requests and their start offsets, bytes handed to the
 * socket, completed and aborted responses). "Bytes written" is what the server handed to the
 * socket: an upper bound of what reached the client, since an aborted response may lose its last
 * buffered chunk.
 */
export async function startFixtureServer(
  bodies,
  { host = "127.0.0.1", port = 0, extra, chunk = 65536, stats } = {},
) {
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://x");
    if (extra && (await extra(req, res, url))) return;
    const body = bodies[url.pathname.slice(1)];
    if (!body) return res.writeHead(404).end();
    const etag = `"${sha256(body).slice(0, 16)}"`;
    let start = 0;
    let end = body.length - 1;
    let status = 200;
    // `?norange=1`: a server without range support (always 200 with the whole body; B2.32).
    const noRange = url.searchParams.get("norange") === "1";
    const range = noRange ? null : /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
    const ifRange = req.headers["if-range"];
    if (range && (!ifRange || ifRange === etag)) {
      start = Number(range[1]);
      end = range[2] ? Number(range[2]) : body.length - 1;
      status = 206;
    }
    const headers = { "Content-Length": end - start + 1, ETag: etag };
    if (!noRange) headers["Accept-Ranges"] = "bytes";
    if (status === 206) headers["Content-Range"] = `bytes ${start}-${end}/${body.length}`;
    res.writeHead(status, headers);
    const rec = stats ? fileStats(stats, url.pathname.slice(1), status, start) : null;
    if (rec) {
      stats.open_responses += 1;
      stats.max_open_responses = Math.max(stats.max_open_responses, stats.open_responses);
    }
    const slow = Number(url.searchParams.get("slow") ?? 0);
    for (let i = start; i <= end; i += chunk) {
      if (res.destroyed) {
        if (rec) {
          rec.aborted_responses += 1;
          stats.open_responses -= 1;
        }
        return;
      }
      const part = body.subarray(i, Math.min(i + chunk, end + 1));
      res.write(part);
      if (rec) {
        rec.bytes_written += part.length;
        stats.total_bytes_written += part.length;
      }
      if (slow) await sleep(slow);
    }
    res.end();
    if (rec) {
      rec.completed_responses += 1;
      stats.open_responses -= 1;
    }
  });
  await new Promise((r) => server.listen(port, host, r));
  return { server, base: `http://${host}:${server.address().port}` };
}
