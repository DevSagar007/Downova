import assert from "node:assert/strict";
import { test } from "node:test";
import { descendants, launcherInfo, parsePs } from "./browser-diagnostics.mjs";

test("ps rows parse and the tree below a launcher is complete, parents first", () => {
  const rows = parsePs(
    [
      "  100     1 Ss      40 node smoke.mjs",
      "  200   100 S       30 /bin/bash /opt/microsoft/msedge/microsoft-edge --headless=new",
      "  201   200 S       30 cat",
      "  300   200 Sl      29 /opt/microsoft/msedge/msedge --type=browser",
      "  301   300 S       29 /opt/microsoft/msedge/msedge --type=zygote",
      "  999     1 S        1 unrelated",
    ].join("\n"),
  );
  assert.equal(rows.length, 6);
  assert.deepEqual(
    descendants(rows, 200).map((r) => r.pid),
    [200, 201, 300, 301],
  );
  assert.equal(rows[3].args, "/opt/microsoft/msedge/msedge --type=browser");
  assert.deepEqual(descendants(rows, 4242), []);
});

test("a shell wrapper is reported with its exec lines; a binary is not a script", () => {
  const info = launcherInfo(
    [
      "#!/bin/bash",
      'HERE="`dirname "$CHROME_WRAPPER"`"',
      "exec < /dev/null",
      "exec > >(exec cat)",
      "exec 2> >(exec cat >&2)",
      'exec -a "$0" "$HERE/msedge" "$@"',
    ].join("\n"),
  );
  assert.equal(info.kind, "script");
  assert.equal(info.shebang, "#!/bin/bash");
  assert.deepEqual(info.exec_lines, [
    "exec < /dev/null",
    "exec > >(exec cat)",
    "exec 2> >(exec cat >&2)",
    'exec -a "$0" "$HERE/msedge" "$@"',
  ]);
  assert.deepEqual(launcherInfo("\u007fELF..."), { kind: "binary" });
});
