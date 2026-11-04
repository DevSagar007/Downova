// Chrome native-messaging framing for test tooling (B2.33): a 32-bit little-endian length, then
// that many bytes of UTF-8 JSON. Used by the fault-injection host (scripts/e2e/fault-host.mjs).
// The product's own framing is in crates/browser-host; this is test infrastructure only.

/** The host → browser limit Chrome documents (1 MiB); also used as the cap for input here. */
export const MAX_MESSAGE = 1024 * 1024;

/** One framed message. */
export function encode(value) {
  const json = Buffer.from(JSON.stringify(value), "utf8");
  if (json.length > MAX_MESSAGE) throw new Error(`message of ${json.length} bytes exceeds the cap`);
  const head = Buffer.alloc(4);
  head.writeUInt32LE(json.length, 0);
  return Buffer.concat([head, json]);
}

/**
 * An incremental decoder: `push(chunk)` returns every message completed by that chunk (chunks
 * may split the length prefix or the body anywhere). An oversized length throws.
 */
export function createDecoder({ maxBytes = MAX_MESSAGE } = {}) {
  let buf = Buffer.alloc(0);
  return {
    push(chunk) {
      buf = Buffer.concat([buf, chunk]);
      const out = [];
      for (;;) {
        if (buf.length < 4) return out;
        const len = buf.readUInt32LE(0);
        if (len > maxBytes) throw new Error(`framed length ${len} exceeds the cap ${maxBytes}`);
        if (buf.length < 4 + len) return out;
        out.push(JSON.parse(buf.subarray(4, 4 + len).toString("utf8")));
        buf = buf.subarray(4 + len);
      }
    },
    /** Bytes received but not yet part of a complete message. */
    pending: () => buf.length,
  };
}
