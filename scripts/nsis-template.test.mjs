// ADR 0039: the template is read out of the Tauri CLI binary on every OS. On macOS and Linux the
// literal is followed by other data, not a NUL (CI run 43835964426), so extraction must stop at
// the template's own end.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { patched, upstreamTemplate } from "./nsis-template.mjs";

const TEMPLATE = [
  "Unicode true",
  "ManifestDPIAware true",
  "; Add in `dpiAwareness` `PerMonitorV2` to manifest",
  "Function CreateOrUpdateDesktopShortcut",
  '  CreateShortcut "$DESKTOP\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe"',
  '  !insertmacro SetLnkAppUserModelId "$DESKTOP\\${PRODUCTNAME}.lnk"',
  "FunctionEnd",
  "",
].join("\r\n");

function binary(after) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nsis-template-"));
  const file = path.join(dir, "cli.node");
  fs.writeFileSync(file, Buffer.concat([Buffer.from([0, 1, 2, 3]), Buffer.from(TEMPLATE), after]));
  return file;
}

test("extraction stops at the template's end whether a NUL follows or other data does", () => {
  const want = TEMPLATE.replace(/\r\n/g, "\n");
  assert.equal(upstreamTemplate(binary(Buffer.from([0, 9, 9]))), want);
  assert.equal(
    upstreamTemplate(binary(Buffer.from("!include other.nsh\r\nSection Other\r\n\0"))),
    want,
  );
});

test("a template without the known ending is refused, not truncated", () => {
  const file = binary(Buffer.alloc(0));
  fs.writeFileSync(file, Buffer.from(TEMPLATE.replace("FunctionEnd", "FunctionEnd2") + "\0"));
  assert.throws(() => upstreamTemplate(file), /end of the NSIS template/);
});

test("each patch must match exactly once", () => {
  assert.throws(() => patched("Unicode true\n"), /expected one match/);
});
