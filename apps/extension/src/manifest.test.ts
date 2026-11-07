import { expect, it } from "vitest";
import { buildManifest, CHROMIUM_DEV_KEY, FIREFOX_DEV_ADDON_ID } from "../manifest";

// Must equal DEV_EXTENSION_ID in crates/browser-host/src/lib.rs (the host's exact allowlist).
const DEV_EXTENSION_ID = "bkmpnfclpdmcbdgmeakmnoilmkdmdppf";

type M = Record<string, unknown> & { permissions?: string[] };

it("the Chromium public key yields the development extension ID the host allows", async () => {
  const der = Uint8Array.from(atob(CHROMIUM_DEV_KEY), (c) => c.charCodeAt(0));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", der));
  const id = Array.from(digest.slice(0, 16), (b) =>
    String.fromCharCode(97 + (b >> 4), 97 + (b & 15)),
  ).join("");
  expect(id).toBe(DEV_EXTENSION_ID);
  expect(buildManifest("chromium").key).toBe(CHROMIUM_DEV_KEY);
});

it("Chromium requests only what the shipped features need; capture and sign-in stay optional", () => {
  const m = buildManifest("chromium") as M;
  expect(m.manifest_version).toBe(3);
  // BRW-10: no private-window capture until an end-to-end ephemeral policy exists.
  expect(m.incognito).toBe("not_allowed");
  expect([...(m.permissions ?? [])].sort()).toEqual(["contextMenus", "nativeMessaging", "storage"]);
  expect(m.optional_permissions).toEqual(["downloads", "webRequest", "cookies"]);
  expect(m.optional_host_permissions).toEqual(["http://*/*", "https://*/*"]);
  expect(m.background).toEqual({ service_worker: "background.js", type: "module" });
  expect(m.browser_specific_settings).toBeUndefined();
  for (const never of ["webRequestBlocking", "downloads.open", "<all_urls>", "debugger"])
    expect(JSON.stringify(m), never).not.toContain(`"${never}"`);
  for (const absent of [
    "host_permissions",
    "content_scripts",
    "externally_connectable",
    "web_accessible_resources",
  ])
    expect(m[absent], absent).toBeUndefined();
});

it("Firefox: its own add-on ID, an event page, and no capture or cookie permissions at all", () => {
  const m = buildManifest("firefox") as M;
  expect(m.manifest_version).toBe(3);
  expect(m.browser_specific_settings).toEqual({
    gecko: {
      id: FIREFOX_DEV_ADDON_ID,
      strict_min_version: "140.0",
      data_collection_permissions: { required: ["browsingActivity"] },
    },
  });
  // Firefox does not run MV3 service workers; an event page runs the same module.
  expect(m.background).toEqual({ scripts: ["background.js"], type: "module" });
  expect([...(m.permissions ?? [])].sort()).toEqual(["contextMenus", "nativeMessaging", "storage"]);
  expect(m.incognito).toBe("not_allowed");
  for (const absent of [
    "key",
    "minimum_chrome_version",
    "optional_permissions",
    "optional_host_permissions",
    "host_permissions",
    "content_scripts",
  ])
    expect(m[absent], absent).toBeUndefined();
  expect(JSON.stringify(m)).not.toContain("cookies");
  // The ID is in the host's allowed_extensions form (email-like, at most 80 characters).
  expect(FIREFOX_DEV_ADDON_ID).toMatch(/^[a-zA-Z0-9-._]*@[a-zA-Z0-9-._]+$/);
  expect(FIREFOX_DEV_ADDON_ID.length).toBeLessThanOrEqual(80);
});

it("only e2e builds grant permissions up front, and only for the local fixture hosts", () => {
  const chromium = buildManifest("chromium", true) as M;
  expect(chromium.optional_permissions).toBeUndefined();
  expect(chromium.permissions).toEqual(
    expect.arrayContaining(["downloads", "webRequest", "cookies"]),
  );
  for (const host of chromium.host_permissions as string[])
    expect(host).toMatch(/^https?:\/\/(127\.0\.0\.1|localhost)\/\*$/);
  // The Firefox e2e build needs no extra permission: its hooks are code, not grants.
  expect(buildManifest("firefox", true)).toEqual(buildManifest("firefox"));
  // Both targets share the user-visible identity.
  for (const k of ["name", "version", "action", "options_ui"])
    expect(buildManifest("firefox")[k], k).toEqual(buildManifest("chromium")[k]);
});
