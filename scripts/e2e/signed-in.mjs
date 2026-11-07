// BRW-05 / SEC-01 real-browser step (B3.3, ADR 0027), used by chrome-handoff.mjs.
//
// An HTTPS fixture on 127.0.0.1 whose test CA the debug service trusts through
// DOWNOVA_TEST_CA_DER (debug builds only); the browser never visits it. The session cookie is put
// into the browser's own cookie store through DevTools (as a sign-in on the site would), and the
// extension's real signed-in path reads it with chrome.cookies for that one link.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import https from "node:https";
import path from "node:path";

export const SIGNED_IN_SECRET = "SYNTH-E2E-SESSION-b33";

function openssl() {
  for (const candidate of [
    "openssl",
    "C:\\Program Files\\Git\\mingw64\\bin\\openssl.exe",
    "C:\\Program Files\\Git\\usr\\bin\\openssl.exe",
  ]) {
    const r = spawnSync(candidate, ["version"], { encoding: "utf8" });
    if (r.status === 0) return candidate;
  }
  return null;
}

/** A throwaway CA and a 127.0.0.1 leaf in `dir`; null when openssl is not available. */
export function makeTestPki(dir) {
  const ssl = openssl();
  if (!ssl) return null;
  fs.mkdirSync(dir, { recursive: true });
  const p = (n) => path.join(dir, n);
  const run = (args) => {
    const r = spawnSync(ssl, args, { encoding: "utf8" });
    if (r.status !== 0) throw new Error(`openssl ${args[0]} failed: ${r.stderr}`);
  };
  fs.writeFileSync(
    p("leaf.ext"),
    "subjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
  );
  run([
    "req",
    "-x509",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-sha256",
    "-days",
    "2",
    "-keyout",
    p("ca.key"),
    "-out",
    p("ca.pem"),
    "-subj",
    "/CN=Downova E2E Test CA",
    "-addext",
    "basicConstraints=critical,CA:TRUE",
    "-addext",
    "keyUsage=critical,keyCertSign,cRLSign",
  ]);
  run([
    "req",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-sha256",
    "-keyout",
    p("leaf.key"),
    "-out",
    p("leaf.csr"),
    "-subj",
    "/CN=127.0.0.1",
  ]);
  run([
    "x509",
    "-req",
    "-in",
    p("leaf.csr"),
    "-CA",
    p("ca.pem"),
    "-CAkey",
    p("ca.key"),
    "-CAcreateserial",
    "-sha256",
    "-days",
    "2",
    "-out",
    p("leaf.pem"),
    "-extfile",
    p("leaf.ext"),
  ]);
  run(["x509", "-in", p("ca.pem"), "-outform", "DER", "-out", p("ca.der")]);
  return {
    caDer: p("ca.der"),
    key: fs.readFileSync(p("leaf.key")),
    cert: fs.readFileSync(p("leaf.pem")),
  };
}

/**
 * Serves `body` at /private/<name> only to requests carrying the session cookie (401 otherwise),
 * and logs every request's path and whether (and which) cookie came with it.
 */
export async function startSignedInServer(pki, files) {
  const log = [];
  const server = https.createServer({ key: pki.key, cert: pki.cert }, (req, res) => {
    const cookie = req.headers.cookie ?? null;
    log.push({ path: req.url, cookie, range: req.headers.range ?? null });
    const name = (req.url ?? "").replace(/^\/private\//, "").split("?")[0];
    const body = files[name];
    if (!body) {
      res.writeHead(404).end();
      return;
    }
    if (cookie !== `sid=${SIGNED_IN_SECRET}`) {
      res.writeHead(401, { "Content-Type": "text/plain" }).end("sign in");
      return;
    }
    const m = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
    if (m) {
      const start = Number(m[1]);
      const end = m[2] ? Number(m[2]) : body.length - 1;
      res.writeHead(206, {
        "Content-Range": `bytes ${start}-${end}/${body.length}`,
        "Content-Length": end - start + 1,
        "Accept-Ranges": "bytes",
        ETag: '"e2e-signed"',
      });
      res.end(body.subarray(start, end + 1));
      return;
    }
    res.writeHead(200, {
      "Content-Length": body.length,
      "Accept-Ranges": "bytes",
      ETag: '"e2e-signed"',
      "Content-Type": "application/octet-stream",
    });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const origin = `https://127.0.0.1:${server.address().port}`;
  return { server, origin, log };
}

/** Files under `dir` (recursively) whose bytes contain `secret`. */
export function filesContaining(dir, secret) {
  const hits = [];
  const needle = Buffer.from(secret);
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else {
        try {
          if (fs.readFileSync(p).includes(needle)) hits.push(p);
        } catch {
          // A file held open exclusively is reported by the caller's later sweep.
        }
      }
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return hits;
}
