import assert from "node:assert/strict";
import { test } from "node:test";
import { checkUpdaterSetup } from "./updater-config.mjs";

const production = {
  bundle: { createUpdaterArtifacts: true },
  plugins: {
    updater: {
      pubkey: "dW50cnVzdGVk",
      endpoints: ["https://updates.example/{{target}}/latest.json"],
      requireSignedVersion: true,
    },
  },
};
const key = { TAURI_SIGNING_PRIVATE_KEY_PATH: "/secure/updater.key" };

test("a normal package has neither an updater config nor signing variables", () => {
  assert.deepEqual(checkUpdaterSetup({ config: null, fixture: false, env: {} }), []);
  assert.equal(checkUpdaterSetup({ config: null, fixture: false, env: key }).length, 1);
  assert.equal(checkUpdaterSetup({ config: null, fixture: true, env: {} }).length, 1);
});

test("a production config needs https, a key, signed-version binding and a signing key", () => {
  assert.deepEqual(checkUpdaterSetup({ config: production, fixture: false, env: key }), []);
  assert.ok(checkUpdaterSetup({ config: production, fixture: false, env: {} }).length > 0);
  const http = structuredClone(production);
  http.plugins.updater.endpoints = ["http://127.0.0.1:9/latest.json"];
  assert.ok(
    checkUpdaterSetup({ config: http, fixture: false, env: key }).some((p) => p.includes("https")),
  );
  const unbound = structuredClone(production);
  unbound.plugins.updater.requireSignedVersion = false;
  assert.ok(checkUpdaterSetup({ config: unbound, fixture: false, env: key }).length > 0);
  const insecure = structuredClone(production);
  insecure.plugins.updater.dangerousInsecureTransportProtocol = true;
  assert.ok(checkUpdaterSetup({ config: insecure, fixture: false, env: key }).length > 0);
  const certs = structuredClone(production);
  certs.plugins.updater.dangerousAcceptInvalidCerts = true;
  assert.ok(checkUpdaterSetup({ config: certs, fixture: true, env: key }).length > 0);
  const noArtifacts = structuredClone(production);
  delete noArtifacts.bundle;
  assert.ok(checkUpdaterSetup({ config: noArtifacts, fixture: false, env: key }).length > 0);
});

test("a fixture config is loopback http only, and only with --update-fixture", () => {
  const fixture = structuredClone(production);
  fixture.plugins.updater.endpoints = ["http://127.0.0.1:8799/latest.json"];
  fixture.plugins.updater.dangerousInsecureTransportProtocol = true;
  assert.deepEqual(checkUpdaterSetup({ config: fixture, fixture: true, env: key }), []);
  assert.ok(checkUpdaterSetup({ config: fixture, fixture: false, env: key }).length > 0);
  const remote = structuredClone(fixture);
  remote.plugins.updater.endpoints = ["http://updates.example/latest.json"];
  assert.ok(checkUpdaterSetup({ config: remote, fixture: true, env: key }).length > 0);
  // A production https endpoint is not a fixture endpoint either.
  assert.ok(checkUpdaterSetup({ config: production, fixture: true, env: key }).length > 0);
});
