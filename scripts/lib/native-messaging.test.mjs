// node --test scripts/lib/native-messaging.test.mjs (part of `pnpm verify`, group structure).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createDecoder, encode } from "./native-messaging.mjs";

test("round trip, byte by byte, with a non-ASCII body", () => {
  const msg = { v: 1, request_id: "req_12345678", url: "http://127.0.0.1/প্রধান.bin" };
  const framed = encode(msg);
  assert.equal(framed.readUInt32LE(0), framed.length - 4);
  const d = createDecoder();
  const got = [];
  for (const byte of framed) got.push(...d.push(Buffer.from([byte])));
  assert.deepEqual(got, [msg]);
  assert.equal(d.pending(), 0);
});

test("two messages in one chunk, and a third split across chunks", () => {
  const [a, b, c] = [{ n: 1 }, { n: 2 }, { n: 3 }].map(encode);
  const d = createDecoder();
  assert.deepEqual(d.push(Buffer.concat([a, b, c.subarray(0, 3)])), [{ n: 1 }, { n: 2 }]);
  assert.equal(d.pending(), 3);
  assert.deepEqual(d.push(c.subarray(3)), [{ n: 3 }]);
});

test("an oversized length is refused before buffering the body", () => {
  const head = Buffer.alloc(4);
  head.writeUInt32LE(64, 0);
  assert.throws(() => createDecoder({ maxBytes: 16 }).push(head), /exceeds the cap/);
  assert.throws(() => encode({ s: "x".repeat(1024 * 1024) }), /exceeds the cap/);
});
