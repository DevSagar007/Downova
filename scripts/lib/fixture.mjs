// Controlled fixture server shared by the packaged smoke, the real-browser test and the manual
// acceptance procedure (B2.6).
//
// Deterministic payloads (xorshift32 from a seed) with a strong ETag, Content-Length and Range
// support (`If-Range` honoured); `?slow=<ms>` delays every 64 KiB so a transfer is still running
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

/**
 * Serves `bodies` (name → Buffer) on `host:port` (0 = any free port). `extra(req, res, url)` may
 * answer other paths first and returns true when it did. Resolves to `{ server, base }`.
 */
export async function startFixtureServer(bodies, { host = "127.0.0.1", port = 0, extra } = {}) {
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://x");
    if (extra && (await extra(req, res, url))) return;
    const body = bodies[url.pathname.slice(1)];
    if (!body) return res.writeHead(404).end();
    const etag = `"${sha256(body).slice(0, 16)}"`;
    let start = 0;
    let end = body.length - 1;
    let status = 200;
    const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
    const ifRange = req.headers["if-range"];
    if (range && (!ifRange || ifRange === etag)) {
      start = Number(range[1]);
      end = range[2] ? Number(range[2]) : body.length - 1;
      status = 206;
    }
    const headers = { "Content-Length": end - start + 1, ETag: etag, "Accept-Ranges": "bytes" };
    if (status === 206) headers["Content-Range"] = `bytes ${start}-${end}/${body.length}`;
    res.writeHead(status, headers);
    const slow = Number(url.searchParams.get("slow") ?? 0);
    for (let i = start; i <= end; i += 65536) {
      if (res.destroyed) return;
      res.write(body.subarray(i, Math.min(i + 65536, end + 1)));
      if (slow) await sleep(slow);
    }
    res.end();
  });
  await new Promise((r) => server.listen(port, host, r));
  return { server, base: `http://${host}:${server.address().port}` };
}
