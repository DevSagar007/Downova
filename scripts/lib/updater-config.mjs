// ADV09 (ADR 0030): what scripts/package.mjs accepts as updater configuration.
//
// - A normal package has no updater config and no updater signing variables in its environment
//   (so a test key can never end up signing a normal package by accident).
// - A production updater config (`--updater-config FILE`) has HTTPS endpoints only, a public key,
//   `requireSignedVersion: true`, no `dangerous*` flag, and `bundle.createUpdaterArtifacts: true`;
//   building it needs the signing key in TAURI_SIGNING_PRIVATE_KEY or _PATH.
// - A fixture config is accepted only with `--update-fixture` (which also compiles the shell's
//   `update-fixture` feature): loopback HTTP endpoints, the insecure-transport flag allowed.

export const SIGNING_VARS = [
  "TAURI_SIGNING_PRIVATE_KEY",
  "TAURI_SIGNING_PRIVATE_KEY_PATH",
  "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
];

const LOOPBACK = new Set(["127.0.0.1", "localhost", "[::1]"]);

/** Returns a list of problems (empty: acceptable). */
export function checkUpdaterSetup({ config, fixture, env }) {
  const problems = [];
  const signing = SIGNING_VARS.filter((v) => env[v]);
  if (!config) {
    if (fixture) problems.push("--update-fixture needs --updater-config");
    if (signing.length > 0)
      problems.push(
        `updater signing variables are set (${signing.join(", ")}) for a package without an updater config`,
      );
    return problems;
  }
  const u = config?.plugins?.updater;
  if (!u || typeof u !== "object") return ["the updater config has no plugins.updater section"];
  if (typeof u.pubkey !== "string" || u.pubkey.trim() === "") problems.push("pubkey is missing");
  if (u.requireSignedVersion !== true) problems.push("requireSignedVersion must be true");
  if (u.dangerousAcceptInvalidCerts || u.dangerousAcceptInvalidHostnames)
    problems.push("invalid TLS certificates or hostnames are never accepted");
  if (u.dangerousInsecureTransportProtocol && !fixture)
    problems.push("insecure transport is allowed only in a fixture build");
  const endpoints = Array.isArray(u.endpoints) ? u.endpoints : [];
  if (endpoints.length === 0) problems.push("no endpoints");
  for (const e of endpoints) {
    let url;
    try {
      url = new URL(e);
    } catch {
      problems.push(`not a URL: ${e}`);
      continue;
    }
    if (fixture) {
      if (url.protocol !== "http:" || !LOOPBACK.has(url.hostname))
        problems.push(`a fixture endpoint must be loopback http: ${url.origin}`);
    } else if (url.protocol !== "https:") {
      problems.push(`a production endpoint must use https: ${url.origin}`);
    }
  }
  if (config?.bundle?.createUpdaterArtifacts !== true)
    problems.push("bundle.createUpdaterArtifacts must be true");
  if (!env.TAURI_SIGNING_PRIVATE_KEY && !env.TAURI_SIGNING_PRIVATE_KEY_PATH)
    problems.push(
      "updater artifacts need TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH",
    );
  return problems;
}
