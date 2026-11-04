// node --test scripts/lib/zip.test.mjs (part of `pnpm verify`, group structure).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { entriesFromDir, readZip, writeZip } from "./zip.mjs";

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), "downova-zip-"));

test("round trip: names, bytes and permissions survive; incompressible data is stored", () => {
  const d = tmp();
  const random = randomBytes(64 * 1024);
  const text = Buffer.from("প্রধান\n".repeat(2000), "utf8");
  const file = path.join(d, "a.zip");
  writeZip(file, [
    { name: "kit/verify-kit.mjs", data: text, mode: 0o755 },
    { name: "kit/package/x.AppImage", data: random, mode: 0o755 },
    { name: "kit/RESULTS.csv", data: Buffer.alloc(0) },
  ]);
  const back = readZip(file);
  assert.deepEqual(
    back.map((e) => [e.name, e.mode]),
    [
      ["kit/verify-kit.mjs", 0o755],
      ["kit/package/x.AppImage", 0o755],
      ["kit/RESULTS.csv", 0o644],
    ],
  );
  assert.ok(back[0].data.equals(text));
  assert.ok(back[1].data.equals(random));
  assert.equal(back[2].data.length, 0);
  fs.rmSync(d, { recursive: true, force: true });
});

test("the same files give the same archive bytes", () => {
  const d = tmp();
  const entries = [{ name: "k/a.txt", data: Buffer.from("same") }];
  writeZip(path.join(d, "1.zip"), entries);
  writeZip(path.join(d, "2.zip"), entries);
  assert.ok(fs.readFileSync(path.join(d, "1.zip")).equals(fs.readFileSync(path.join(d, "2.zip"))));
  fs.rmSync(d, { recursive: true, force: true });
});

test("unsafe or duplicate names are refused", () => {
  const d = tmp();
  for (const name of ["/abs", "C:/x", "a/../b", "a\\b", "a//b", ""])
    assert.throws(
      () => writeZip(path.join(d, "x.zip"), [{ name, data: Buffer.from("x") }]),
      /unsafe/,
    );
  assert.throws(
    () =>
      writeZip(path.join(d, "x.zip"), [
        { name: "a", data: Buffer.from("1") },
        { name: "a", data: Buffer.from("2") },
      ]),
    /duplicate/,
  );
  fs.rmSync(d, { recursive: true, force: true });
});

test("a damaged archive is detected", () => {
  const d = tmp();
  const file = path.join(d, "a.zip");
  writeZip(file, [{ name: "k/a.bin", data: Buffer.from("payload that is stored as is") }]);
  const buf = fs.readFileSync(file);
  buf[30 + "k/a.bin".length + 3] ^= 0xff; // one byte of the stored body
  fs.writeFileSync(file, buf);
  assert.throws(() => readZip(file), /CRC mismatch/);
  fs.writeFileSync(file, Buffer.from("not a zip"));
  assert.throws(() => readZip(file), /not a zip/);
  fs.rmSync(d, { recursive: true, force: true });
});

test("entriesFromDir keeps relative paths and marks executables", () => {
  const d = tmp();
  fs.mkdirSync(path.join(d, "kit", "fixtures", "lib"), { recursive: true });
  fs.writeFileSync(path.join(d, "kit", "verify-kit.mjs"), "v");
  fs.writeFileSync(path.join(d, "kit", "fixtures", "lib", "fixture.mjs"), "f");
  const e = entriesFromDir(path.join(d, "kit"), "kit", (rel) => rel === "verify-kit.mjs");
  assert.deepEqual(
    e.map((x) => [x.name, x.mode]),
    [
      ["kit/fixtures/lib/fixture.mjs", 0o644],
      ["kit/verify-kit.mjs", 0o755],
    ],
  );
  fs.rmSync(d, { recursive: true, force: true });
});

test(
  "the platform's standard extractor reads the archive",
  { skip: process.platform !== "win32" && !spawnSync("unzip", ["-v"]).stdout ? "no unzip" : false },
  () => {
    const d = tmp();
    const file = path.join(d, "a.zip");
    writeZip(file, [
      { name: "kit/sub/a.txt", data: Buffer.from("hello") },
      { name: "kit/run.mjs", data: Buffer.from("x"), mode: 0o755 },
    ]);
    const out = path.join(d, "out");
    fs.mkdirSync(out);
    const r =
      process.platform === "win32"
        ? spawnSync(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe"), [
            "-x",
            "-f",
            file,
            "-C",
            out,
          ])
        : spawnSync("unzip", ["-q", file, "-d", out]);
    assert.equal(r.status, 0, String(r.stderr));
    assert.equal(fs.readFileSync(path.join(out, "kit/sub/a.txt"), "utf8"), "hello");
    if (process.platform !== "win32")
      assert.ok(fs.statSync(path.join(out, "kit/run.mjs")).mode & 0o100);
    fs.rmSync(d, { recursive: true, force: true });
  },
);
