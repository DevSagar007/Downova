#!/usr/bin/env node
// B2.6 hands-on acceptance: the controlled fixture server with a page of links to right-click.
//
//   node scripts/acceptance/fixture-server.mjs [--port 8765]
//
// Open the printed page in Chrome or Edge, right-click a link and choose "Download with Downova".
// The server logs every request (path, Range, status) so a resume after "Exit and stop downloads"
// can be seen as a ranged request. Ctrl+C stops it. Payloads are deterministic: the SHA-256 values
// printed here are what the downloaded files must hash to (docs/reports/b2.6-acceptance.md).
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

const page = (base) => `<!doctype html>
<meta charset="utf-8"><title>Downova acceptance fixtures</title>
<style>body{font:16px system-ui;margin:2rem;max-width:48rem}code{font-size:13px}li{margin:1rem 0}</style>
<h1>Downova acceptance fixtures</h1>
<p>Right-click a link and choose <b>Download with Downova</b>. Do not left-click.</p>
<ul>
${files
  .map(
    (f) =>
      `<li><a href="${base}/${f.name}?slow=${f.slow}">${f.name}</a> (${f.size / MiB} MiB, about ${Math.round(
        ((f.size / 65536) * f.slow) / 1000,
      )} s)<br>SHA-256 <code>${sha256(bodies[f.name])}</code></li>`,
  )
  .join("\n")}
</ul>`;

let base = "";
({ base } = await startFixtureServer(bodies, {
  port,
  extra: (req, res, url) => {
    // Every request is logged (path without query values, Range), then served normally.
    const range = req.headers.range ? ` Range: ${req.headers.range}` : "";
    console.log(`${new Date().toISOString()} ${req.method} ${url.pathname}${range}`);
    if (url.pathname !== "/") return false;
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(page(base));
    return true;
  },
}));
console.log(`Fixture page: ${base}/`);
for (const f of files) console.log(`  ${f.name}  SHA-256 ${sha256(bodies[f.name])}`);
console.log("Requests:");
