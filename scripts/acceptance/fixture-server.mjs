#!/usr/bin/env node
// Hands-on acceptance (B2.6, extended for B2.15–B2.20): a controlled fixture server with a page of
// deterministic scenarios.
//
//   node scripts/acceptance/fixture-server.mjs [--port 8765]
//
// Open the printed page in Chrome or Edge. Every request is logged (path without query values,
// Range, status) so continuations can be seen as ranged requests. Ctrl+C stops it. Payloads are
// deterministic: the SHA-256 values printed here are what the downloaded files must hash to.
//
// Scenarios (state is per server run; "Reset scenarios" re-arms them):
//   A/B  long downloads for close, exit and resume (B2.6);
//   P    a public download, plus browser-only fallbacks (POST form, blob:, a cookie-carrying link);
//   R    a link that fails once (HTTP 412) and works afterwards → Retry;
//   E    a signed link that expires on demand (403 for the old token) → Refresh link with the new
//        token keeps the progress (same file, same ETag);
//   C    a file that changes on demand (same length, new ETag) → Refresh link needs Restart;
//   S    publisher checksums: one that matches and one that does not.
import { payload, sha256, startFixtureServer } from "../lib/fixture.mjs";

const argv = process.argv.slice(2);
const i = argv.indexOf("--port");
const port = i >= 0 ? Number(argv[i + 1]) : 8765;

const MiB = 1024 * 1024;
// a: completes in ~30 s, so the window can be closed while it runs.
// b: ~60 s, so "Exit and stop downloads" catches it unfinished and Resume continues it.
const files = [
  { name: "downova-acceptance-a.bin", size: 32 * MiB, seed: 2026, slow: 60 },
  { name: "downova-acceptance-b.bin", size: 32 * MiB, seed: 2027, slow: 120 },
];
const bodies = Object.fromEntries(files.map((f) => [f.name, payload(f.size, f.seed)]));
// B2.15–B2.20 scenarios.
bodies["public.bin"] = payload(8 * MiB, 3001);
bodies["retry.bin"] = payload(4 * MiB, 3002);
const expiring = payload(16 * MiB, 3003);
const changingA = payload(16 * MiB, 3004);
const changingB = payload(16 * MiB, 3005);
bodies["checksum.bin"] = payload(2 * MiB, 3006);
const wrongChecksum = sha256(payload(2 * MiB, 3007));

const state = { retryFailed: false, token: 1, changed: false };
const reset = () => Object.assign(state, { retryFailed: false, token: 1, changed: false });

const row = (label, href, body, note) =>
  `<li><a href="${href}">${label}</a>${note ? ` — ${note}` : ""}<br>SHA-256 <code>${sha256(body)}</code></li>`;

const page = (base) => `<!doctype html>
<meta charset="utf-8"><title>Downova acceptance fixtures</title>
<style>body{font:16px system-ui;margin:2rem;max-width:52rem}code{font-size:13px;overflow-wrap:anywhere}
li{margin:1rem 0}h2{margin-top:2rem}.ctl{display:inline-block;margin:.25rem 0}</style>
<h1>Downova acceptance fixtures</h1>
<p>Right-click a link and choose <b>Download with Downova</b> unless a step says otherwise.
<a class="ctl" href="/control/reset">Reset scenarios</a></p>

<h2>A/B. Close, exit and resume</h2>
<ul>
${files
  .map((f) =>
    row(
      f.name,
      `${base}/${f.name}?slow=${f.slow}`,
      bodies[f.name],
      `${f.size / MiB} MiB, about ${Math.round(((f.size / 65536) * f.slow) / 1000)} s`,
    ),
  )
  .join("\n")}
</ul>

<h2>P. Public download and browser fallback</h2>
<ul>
${row("public.bin", `${base}/public.bin?slow=20`, bodies["public.bin"], "a plain public GET: with automatic capture on for this host, Downova takes it over after accepting it")}
<li><form method="post" action="${base}/export.csv"><button>Export (POST form)</button></form> — always stays in the browser (reason: form/POST)</li>
<li><button id="blob">Save generated text (blob:)</button> — always stays in the browser</li>
<li><a href="/control/cookie">Set a cookie</a>, then left-click <a href="${base}/public.bin?with=cookie">public.bin with cookie</a> — stays in the browser (cookies were sent)</li>
</ul>

<h2>R. Failed download, then Retry</h2>
<ul>
${row("retry.bin", `${base}/retry.bin`, bodies["retry.bin"], 'the first request fails (HTTP 412, "the file changed"); in Downova use Retry: the same download completes with this hash')}
</ul>

<h2>E. Expired link, then a compatible refresh</h2>
<ol>
<li>Right-click <a href="${base}/expiring.bin?token=${state.token}&slow=120">expiring.bin (current link)</a> and let it run a few seconds.</li>
<li>Pause it in Downova, then <a class="ctl" href="/control/expire">expire the link</a> (the old token now answers 403).</li>
<li>Reload this page and copy the new link: <code>${base}/expiring.bin?token=${state.token}</code></li>
<li>In Downova: Refresh link… with the new link. Expected: the progress is kept; Resume completes it.</li>
</ol>
<p>SHA-256 <code>${sha256(expiring)}</code></p>

<h2>C. Changed file, explicit restart</h2>
<ol>
<li>Right-click <a href="${base}/changing.bin?slow=120">changing.bin</a>, let it run, then pause it in Downova.</li>
<li><a class="ctl" href="/control/change">Change the file</a> (same size, different content and ETag).</li>
<li>Refresh link… with <code>${base}/changing.bin?v=2</code>. Expected: "The progress cannot be kept"; Keep current changes nothing; Restart from zero completes with the new hash.</li>
</ol>
<p>Before the change SHA-256 <code>${sha256(changingA)}</code><br>After the change SHA-256 <code>${sha256(changingB)}</code></p>

<h2>S. Publisher checksum</h2>
<ul>
<li>Add URL <code>${base}/checksum.bin</code> with SHA-256 <code>${sha256(bodies["checksum.bin"])}</code> → "Publisher checksum matched".</li>
<li>Add URL <code>${base}/checksum.bin?again=1</code> with SHA-256 <code>${wrongChecksum}</code> → fails "does not match the expected checksum"; no file is placed.</li>
</ul>
<script>
document.getElementById("blob").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob(["generated by the page\\n"], { type: "text/plain" }));
  a.download = "generated.txt";
  a.click();
};
</script>`;

/** Serves `body` with ranges and a strong ETag (the shared fixture logic, for computed bodies). */
async function serve(req, res, body, etag, slow) {
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
  for (let at = start; at <= end; at += 65536) {
    if (res.destroyed) return;
    res.write(body.subarray(at, Math.min(at + 65536, end + 1)));
    if (slow) await new Promise((r) => setTimeout(r, slow));
  }
  res.end();
}

const back = (res) => res.writeHead(303, { Location: "/" }).end();

let base = "";
({ base } = await startFixtureServer(bodies, {
  port,
  extra: async (req, res, url) => {
    const range = req.headers.range ? ` Range: ${req.headers.range}` : "";
    const respond = (status) =>
      console.log(`${new Date().toISOString()} ${req.method} ${url.pathname}${range} → ${status}`);
    const slow = Number(url.searchParams.get("slow") ?? 0);
    switch (url.pathname) {
      case "/":
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(page(base));
        return true;
      case "/control/reset":
        reset();
        respond(303);
        back(res);
        return true;
      case "/control/expire":
        state.token += 1;
        respond(303);
        back(res);
        return true;
      case "/control/change":
        state.changed = true;
        respond(303);
        back(res);
        return true;
      case "/control/cookie":
        res
          .writeHead(303, { Location: "/", "Set-Cookie": "acceptance=1; Path=/; SameSite=Lax" })
          .end();
        respond(303);
        return true;
      case "/export.csv":
        respond(200);
        res
          .writeHead(200, {
            "Content-Type": "text/csv",
            "Content-Disposition": 'attachment; filename="export.csv"',
          })
          .end("a,b\n1,2\n");
        return true;
      case "/retry.bin":
        if (!state.retryFailed) {
          state.retryFailed = true;
          respond(412);
          res.writeHead(412).end();
          return true;
        }
        respond(200);
        return false;
      case "/expiring.bin":
        if (url.searchParams.get("token") !== String(state.token)) {
          respond(403);
          res.writeHead(403).end();
          return true;
        }
        respond(req.headers.range ? 206 : 200);
        await serve(req, res, expiring, `"exp-${sha256(expiring).slice(0, 12)}"`, slow);
        return true;
      case "/changing.bin": {
        const body = state.changed ? changingB : changingA;
        respond(req.headers.range ? 206 : 200);
        await serve(req, res, body, `"chg-${sha256(body).slice(0, 12)}"`, slow);
        return true;
      }
      default:
        respond("served");
        return false;
    }
  },
}));
console.log(`Fixture page: ${base}/`);
for (const f of files) console.log(`  ${f.name}  SHA-256 ${sha256(bodies[f.name])}`);
console.log(`  public.bin    SHA-256 ${sha256(bodies["public.bin"])}`);
console.log(`  retry.bin     SHA-256 ${sha256(bodies["retry.bin"])}`);
console.log(`  expiring.bin  SHA-256 ${sha256(expiring)}`);
console.log(`  changing.bin  before ${sha256(changingA)}  after ${sha256(changingB)}`);
console.log(
  `  checksum.bin  SHA-256 ${sha256(bodies["checksum.bin"])} (wrong one shown: ${wrongChecksum})`,
);
console.log("Requests:");
