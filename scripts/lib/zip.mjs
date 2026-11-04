// Minimal ZIP writer and reader for the acceptance kit (B2.35). Node built-ins only.
//
// Why not `tar -a`: Windows' bsdtar and macOS's bsdtar write ZIP that way, but GNU tar on Linux cannot
// (and a GNU tar earlier on PATH on Windows breaks it too). This writer behaves the same everywhere:
// - deflate (or store when deflate does not help), CRC-32 from zlib;
// - Unix permissions in the external attributes (made by "UNIX"), so `verify-kit.mjs` and the
//   fixtures keep 0755 and the rest 0644 when extracted with `unzip` on macOS and Linux;
// - one fixed timestamp (2026-01-01 00:00), so the same files always give the same archive;
// - forward-slash names, no absolute paths, no `..`, no directories entries needed.
// No ZIP64: the writer refuses an archive or entry of 4 GiB or more (the kit is ~15 MB).
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const DOS_TIME = 0; // 00:00:00
const DOS_DATE = ((2026 - 1980) << 9) | (1 << 5) | 1; // 2026-01-01
const LIMIT = 0xffffffff;

function safeName(name) {
  if (
    !name ||
    name.startsWith("/") ||
    /^[A-Za-z]:/.test(name) ||
    name.includes("\\") ||
    name.split("/").some((p) => p === ".." || p === "")
  )
    throw new Error(`unsafe zip entry name: ${JSON.stringify(name)}`);
  return name;
}

/**
 * @param {string} file the .zip to write
 * @param {{ name: string, data: Buffer, mode?: number }[]} entries
 */
export function writeZip(file, entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  const seen = new Set();
  for (const { name, data, mode = 0o644 } of entries) {
    safeName(name);
    if (seen.has(name)) throw new Error(`duplicate zip entry: ${name}`);
    seen.add(name);
    if (data.length >= LIMIT) throw new Error(`${name}: entries of 4 GiB or more need ZIP64`);
    const nameBuf = Buffer.from(name, "utf8");
    const crc = zlib.crc32(data) >>> 0;
    const deflated = zlib.deflateRawSync(data, { level: 9 });
    const useDeflate = deflated.length < data.length;
    const body = useDeflate ? deflated : data;
    const method = useDeflate ? 8 : 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(method, 8);
    local.writeUInt16LE(DOS_TIME, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE((3 << 8) | 20, 4); // made by UNIX, spec 2.0
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt16LE(DOS_TIME, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(body.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE((((0o100000 | mode) & 0xffff) << 16) >>> 0, 38); // regular file + mode
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, body);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + body.length;
    if (offset >= LIMIT) throw new Error("archives of 4 GiB or more need ZIP64");
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  fs.writeFileSync(file, Buffer.concat([...locals, cd, end]));
}

/** All regular files under `dir` as entries named relative to `prefix/`, executable bits kept. */
export function entriesFromDir(dir, prefix, executable = () => false) {
  const out = [];
  const walk = (d) => {
    for (const e of fs
      .readdirSync(d, { withFileTypes: true })
      .sort((a, b) => a.name.localeCompare(b.name))) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.isFile()) {
        const rel = path.relative(dir, p).split(path.sep).join("/");
        out.push({
          name: `${prefix}/${rel}`,
          data: fs.readFileSync(p),
          mode: executable(rel) ? 0o755 : 0o644,
        });
      } else throw new Error(`not a regular file: ${p}`);
    }
  };
  walk(dir);
  return out;
}

/**
 * Reads a ZIP written by this module or any standard tool (no ZIP64, no encryption): entries with
 * name, data and mode. Every CRC is checked.
 */
export function readZip(file) {
  const buf = fs.readFileSync(file);
  const endAt = buf.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (endAt < 0) throw new Error("not a zip file (no end of central directory)");
  const count = buf.readUInt16LE(endAt + 10);
  let p = buf.readUInt32LE(endAt + 16);
  const entries = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) throw new Error("bad central directory");
    const method = buf.readUInt16LE(p + 10);
    const crc = buf.readUInt32LE(p + 16);
    const csize = buf.readUInt32LE(p + 20);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const attrs = buf.readUInt32LE(p + 38);
    const localAt = buf.readUInt32LE(p + 42);
    const name = buf.subarray(p + 46, p + 46 + nameLen).toString("utf8");
    const lNameLen = buf.readUInt16LE(localAt + 26);
    const lExtraLen = buf.readUInt16LE(localAt + 28);
    const start = localAt + 30 + lNameLen + lExtraLen;
    const raw = buf.subarray(start, start + csize);
    const data = method === 8 ? zlib.inflateRawSync(raw) : method === 0 ? Buffer.from(raw) : null;
    if (!data) throw new Error(`${name}: unsupported compression method ${method}`);
    if (zlib.crc32(data) >>> 0 !== crc) throw new Error(`${name}: CRC mismatch`);
    if (!name.endsWith("/")) entries.push({ name, data, mode: (attrs >>> 16) & 0o777 });
    p += 46 + nameLen + extraLen + commentLen;
  }
  return entries;
}
