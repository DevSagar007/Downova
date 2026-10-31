import { expect, it } from "vitest";
import manifest from "../public/manifest.json";

// Must equal DEV_EXTENSION_ID in crates/browser-host/src/lib.rs (the host's exact allowlist).
const DEV_EXTENSION_ID = "bkmpnfclpdmcbdgmeakmnoilmkdmdppf";

it("the public key yields the development extension ID the host allows", async () => {
  const der = Uint8Array.from(atob(manifest.key), (c) => c.charCodeAt(0));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", der));
  const id = Array.from(digest.slice(0, 16), (b) =>
    String.fromCharCode(97 + (b >> 4), 97 + (b & 15)),
  ).join("");
  expect(id).toBe(DEV_EXTENSION_ID);
});

it("requests only what the shipped features need; capture permissions stay optional", () => {
  expect(manifest.manifest_version).toBe(3);
  // BRW-10: no private-window capture until an end-to-end ephemeral policy exists.
  expect(manifest.incognito).toBe("not_allowed");
  expect([...manifest.permissions].sort()).toEqual(["contextMenus", "nativeMessaging", "storage"]);
  const m = manifest as Record<string, unknown>;
  expect(m.optional_permissions).toEqual(["downloads", "webRequest"]);
  expect(m.optional_host_permissions).toEqual(["http://*/*", "https://*/*"]);
  // Non-blocking observation only; cookies are never read as a capability.
  for (const never of ["webRequestBlocking", "cookies", "downloads.open", "<all_urls>", "debugger"])
    expect(JSON.stringify(manifest), never).not.toContain(`"${never}"`);
  for (const absent of [
    "host_permissions",
    "content_scripts",
    "externally_connectable",
    "web_accessible_resources",
  ])
    expect(m[absent], absent).toBeUndefined();
});
