import assert from "node:assert/strict";
import { test } from "node:test";
import {
  NOTICE_FILE,
  checkInventory,
  classifyCargo,
  renderText,
  textId,
  unresolvedFor,
} from "./notices.mjs";

const pkg = (name, kind = ["lib"]) => ({ id: name, name, version: "1.0.0", targets: [{ kind }] });
const edge = (pkg, kind = null) => ({ pkg, dep_kinds: [{ kind, target: null }] });

test("normal edges are distributed; proc-macros, build deps and dev deps are not", () => {
  const metadata = {
    workspace_members: ["dm-desktop", "dm-service", "dm-cli", "dm-browser-host", "dm-domain"],
    packages: [
      ...["dm-desktop", "dm-service", "dm-cli", "dm-browser-host", "dm-domain"].map((n) => pkg(n)),
      pkg("serde"),
      pkg("serde_derive", ["proc-macro"]),
      pkg("syn"),
      pkg("cc"),
      pkg("tempfile"),
      pkg("itoa"),
    ],
    resolve: {
      nodes: [
        {
          id: "dm-desktop",
          deps: [edge("dm-domain"), edge("cc", "build"), edge("tempfile", "dev")],
        },
        { id: "dm-domain", deps: [edge("serde")] },
        { id: "serde", deps: [edge("serde_derive"), edge("itoa")] },
        { id: "serde_derive", deps: [edge("syn")] },
        { id: "dm-service", deps: [] },
        { id: "dm-cli", deps: [] },
        { id: "dm-browser-host", deps: [] },
      ],
    },
  };
  const { distributed, buildOnly } = classifyCargo(metadata);
  assert.deepEqual([...distributed].sort(), ["itoa", "serde"]);
  assert.deepEqual([...buildOnly].sort(), ["cc", "serde_derive", "syn"]);
  assert.ok(!distributed.has("tempfile") && !buildOnly.has("tempfile"));
  assert.ok(!distributed.has("dm-domain"), "own workspace code is not a third-party component");
});

test("a missing shipped root is an error, not a smaller inventory", () => {
  assert.throws(() =>
    classifyCargo({ workspace_members: [], packages: [], resolve: { nodes: [] } }),
  );
});

test("license files are recognised and missing metadata is recorded, never filled in", () => {
  for (const f of [
    "LICENSE",
    "LICENSE-MIT",
    "license-apache-2.0",
    "COPYING",
    "NOTICE",
    "UNLICENSE",
    "LICENSE.md",
  ])
    assert.ok(NOTICE_FILE.test(f), f);
  for (const f of ["README.md", "Cargo.toml", "licenses.rs"]) assert.ok(!NOTICE_FILE.test(f), f);
  assert.deepEqual(unresolvedFor({ license: "MIT", texts: ["x"] }), []);
  assert.equal(unresolvedFor({ license: null, texts: [] }).length, 2);
  assert.equal(textId("a\r\nb\r\n"), textId("a\nb\n"));
});

test("the inventory check refuses unrecorded gaps and dangling texts", () => {
  const ok = {
    components: [
      {
        name: "a",
        version: "1",
        ecosystem: "cargo",
        license: "MIT",
        targets: ["linux-x86_64"],
        texts: ["t1"],
        unresolved: [],
      },
    ],
    texts: { t1: { file: "a/LICENSE", text: "MIT\n", used_by: ["a 1"] } },
  };
  assert.deepEqual(checkInventory(ok), []);
  const gap = structuredClone(ok);
  gap.components[0].license = null;
  assert.equal(checkInventory(gap).length, 1);
  const dangling = structuredClone(ok);
  dangling.components[0].texts = ["missing"];
  assert.ok(checkInventory(dangling).length >= 1);
  const noTargets = structuredClone(ok);
  noTargets.components[0].targets = [];
  assert.equal(checkInventory(noTargets).length, 1);
});

test("the bundled text keeps license texts verbatim and marks unknowns", () => {
  const text = renderText({
    inputs: { cargo_lock_sha256: "a".repeat(64), pnpm_lock_sha256: "b".repeat(64) },
    components: [
      {
        name: "a",
        version: "1",
        license: null,
        source: "s",
        repository: null,
        targets: ["windows-x86_64"],
        texts: ["t1"],
        unresolved: ["no license expression in the package metadata"],
      },
    ],
    external: [{ name: "aria2", version: "1.37.0", reason: "not bundled" }],
    texts: {
      t1: { file: "a/LICENSE", text: "Copyright (c) Ä\n\n  indented line\n", used_by: ["a 1"] },
    },
  });
  assert.match(text, /License: UNKNOWN/);
  assert.match(text, /UNRESOLVED: no license expression/);
  assert.match(text, /Copyright \(c\) Ä\n\n {2}indented line\n/);
  assert.match(text, /aria2 1\.37\.0: not bundled/);
});
