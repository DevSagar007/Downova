// node --test scripts/lib/first-line.test.mjs (part of `pnpm verify`, group test).
import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { spawn } from "node:child_process";
import { readFirstJsonLine, readFirstLine } from "./first-line.mjs";

/** A fake child: a stdout stream plus the `exit` event. */
function fakeChild() {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  return child;
}

test("a line split over several chunks is joined", async () => {
  const c = fakeChild();
  const p = readFirstJsonLine(c, { timeoutMs: 2000 });
  c.stdout.write('{"base":"http://127.0.0.1:');
  c.stdout.write('1234","sha256":{"f0.bin":"ab"}}\r');
  c.stdout.write("\nsecond line\n");
  assert.deepEqual(await p, { base: "http://127.0.0.1:1234", sha256: { "f0.bin": "ab" } });
});

test("a multi-byte character split between chunks survives", async () => {
  const c = fakeChild();
  const p = readFirstLine(c, { timeoutMs: 2000 });
  const bytes = Buffer.from("প্রধান\n", "utf8");
  c.stdout.write(bytes.subarray(0, 2));
  c.stdout.write(bytes.subarray(2));
  assert.equal(await p, "প্রধান");
});

test("listeners are removed once the line is read", async () => {
  const c = fakeChild();
  const p = readFirstLine(c, { timeoutMs: 2000 });
  c.stdout.write("one\n");
  await p;
  assert.equal(c.stdout.listenerCount("data"), 0);
  assert.equal(c.listenerCount("exit"), 0);
});

test("stdout ending without a newline rejects", async () => {
  const c = fakeChild();
  const p = readFirstLine(c, { timeoutMs: 2000 });
  c.stdout.write("partial");
  c.stdout.end();
  await assert.rejects(p, /ended before a complete line.*partial/);
});

test("an exit with no line rejects after the grace period", async () => {
  const c = fakeChild();
  const p = readFirstLine(c, { timeoutMs: 10_000 });
  c.emit("exit", 3, null);
  await assert.rejects(p, /exited \(3\)/);
});

test("no line before the deadline rejects", async () => {
  const c = fakeChild();
  await assert.rejects(readFirstLine(c, { timeoutMs: 50 }), /within 50 ms/);
});

test("a malformed first line names the line", async () => {
  const c = fakeChild();
  const p = readFirstJsonLine(c, { timeoutMs: 2000 });
  c.stdout.write("listening\n");
  await assert.rejects(p, /not JSON: "listening"/);
});

test("a real process that prints and exits at once is still read", async () => {
  const child = spawn(process.execPath, ["-e", "process.stdout.write('{\"ok\":1}\\n')"], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  assert.deepEqual(await readFirstJsonLine(child, { timeoutMs: 10_000 }), { ok: 1 });
});

test("a real process that exits without output rejects", async () => {
  const child = spawn(process.execPath, ["-e", "process.exit(2)"], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  await assert.rejects(readFirstLine(child, { timeoutMs: 10_000 }), /before a complete line/);
});
