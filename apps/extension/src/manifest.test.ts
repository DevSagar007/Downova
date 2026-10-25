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

it("requests only what the explicit hand-off needs", () => {
  expect(manifest.manifest_version).toBe(3);
  expect(manifest.incognito).toBe("not_allowed");
  expect([...manifest.permissions].sort()).toEqual(["contextMenus", "nativeMessaging", "storage"]);
  const m = manifest as Record<string, unknown>;
  for (const absent of [
    "host_permissions",
    "optional_permissions",
    "optional_host_permissions",
    "content_scripts",
    "externally_connectable",
    "web_accessible_resources",
  ])
    expect(m[absent], absent).toBeUndefined();
});
