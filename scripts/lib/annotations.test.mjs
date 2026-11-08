// node --test scripts/lib/annotations.test.mjs (part of `pnpm verify`, group structure)
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ANNOTATION_BYTES,
  MAX_DETAIL_ANNOTATIONS,
  annotationLines,
  logTailAnnotation,
  clip,
  rowId,
} from "./annotations.mjs";

const decode = (line) =>
  line
    .slice(line.indexOf("::", 2) + 2)
    .replace(/%0A/g, "\n")
    .replace(/%0D/g, "\r")
    .replace(/%25/g, "%");

// A run shaped like the real smoke: ~96 long rows, the failure late in the list (runs #30–#36 hid it).
function bigRun(failAt = 80) {
  const steps = Array.from({ length: 96 }, (_, i) => ({
    step: `X${i} ${"a long description of what this row proves, ".repeat(2)}`,
    status: i === failAt ? "failed" : i === 3 ? "note" : "passed",
    detail_value: i,
  }));
  return { verdict: "failed", steps };
}

test("the summary lists every non-passed row first and stays under GitHub's limit", () => {
  const [summary] = annotationLines("Packaged smoke linux", bigRun());
  assert.match(summary, /^::error title=Packaged smoke linux::/);
  const text = decode(summary);
  assert.ok(Buffer.byteLength(text) <= ANNOTATION_BYTES, `${Buffer.byteLength(text)} bytes`);
  const lines = text.split("\n");
  assert.match(lines[0], /^verdict failed; 94 passed, 1 note, 1 failed$/);
  assert.match(lines[1], /^NOTE X3 /);
  assert.match(lines[2], /^FAILED X80 /);
  assert.match(lines[3], /^PASSED \(94\): X0 X1 X2 X4 /);
  assert.ok(lines[3].includes(" X95"), "every passed row is named");
});

test("each non-passed row gets its own annotation with its detail", () => {
  const out = annotationLines("Packaged smoke linux", bigRun());
  assert.equal(out.length, 3);
  assert.match(out[1], /^::warning title=Packaged smoke linux — X3::/);
  assert.match(out[2], /^::error title=Packaged smoke linux — X80::/);
  assert.match(decode(out[2]), /"detail_value":80/);
});

test("detail annotations are bounded in size and count", () => {
  const steps = Array.from({ length: 20 }, (_, i) => ({
    step: `F${i} fails`,
    status: "failed",
    blob: "x".repeat(10_000),
  }));
  const out = annotationLines("t", { verdict: "failed", steps });
  assert.equal(out.length, 1 + MAX_DETAIL_ANNOTATIONS);
  for (const line of out) assert.ok(Buffer.byteLength(decode(line)) <= ANNOTATION_BYTES);
  assert.match(decode(out[1]), /…\[cut\]$/);
});

test("a passing run is one notice", () => {
  const out = annotationLines("t", {
    verdict: "passed",
    steps: [{ step: "A1 ok", status: "passed" }],
  });
  assert.deepEqual(out.length, 1);
  assert.match(out[0], /^::notice /);
});

test("clipping never splits a multi-byte character and row IDs are short", () => {
  const bangla = "দলিল".repeat(2000);
  const cut = clip(bangla, 100);
  assert.ok(Buffer.byteLength(cut) <= 100);
  assert.ok(!cut.includes("�"));
  assert.equal(rowId("R3b Chrome hand-off"), "R3b");
  assert.equal(rowId("D2 this installation's registration"), "D2");
  assert.equal(rowId("P2 the packaged list"), "P2");
  assert.equal(rowId("G a closing step"), "G");
});

test("a failed build log becomes one bounded annotation, telling lines first", () => {
  const log = [
    ...Array.from({ length: 500 }, (_, i) => `   Compiling crate${i} v1.0.0`),
    "Downloading https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip",
    "failed to bundle project: http status: 504",
    "package: npx failed with 1",
  ].join("\r\n");
  const a = logTailAnnotation("package windows-x86_64: build failed", log);
  assert.match(a, /^::error title=package windows-x86_64 {2}build failed::/);
  assert.match(a, /http status {2}504|http status: 504/);
  assert.ok(Buffer.byteLength(a) < 4096 + 200, "under GitHub's limit");
  assert.ok(!a.includes("\n") && !a.includes("\r"), "escaped to one line");
  assert.ok(a.indexOf("http status") < a.indexOf("--- last lines ---"), "telling lines first");
});
