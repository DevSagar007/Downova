// The extension's manifests (ADR 0029): one shared description, two build targets.
//
// - **chromium** (Chrome, Edge): the MV3 service worker, the fixed development key (its ID is the
//   host's allowlisted origin), optional capture and sign-in permissions.
// - **firefox**: an event page (`background.scripts`; Firefox does not run MV3 service workers),
//   an explicit add-on ID (the host's `allowed_extensions`), and only what the Firefox build ships:
//   explicit public hand-offs. No capture or cookie permissions, no sign-in actions.
//
// `e2e` adds what real-browser tests need and normal packages never contain: the optional
// permissions are granted up front (automation cannot answer permission prompts), only for the
// local fixture hosts.

export type Target = "chromium" | "firefox";

/** Must equal `DEV_EXTENSION_ID`'s key in crates/browser-host/src/lib.rs. */
export const CHROMIUM_DEV_KEY =
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0TA6GnRPmYHGtInXhTtsxab0j7LmWidd6y6SQRSvsZT76YJZL/2PiAJIGD4fFmaUL9mNGOOb4OU72YFEpN886QN9y8riOAAIhGxcMpxuFMFnprscmzE8c9arN4RDpr6tjjCP9xTMmGIuJCDrbKK6e152PU7F8Y3FXlqvEA5prGRJxT0K+kah6iGbzRAEhjLV9+muWjzEJHQSRoiYjddc06tqEdPXhPIf+DyRFWS3W03J0Q4woiB9J++M8ITgYrxKCgl5q7Fgg8621O0wb5G0t1/0NpVUtfjH5emDIBCqpbU7EAG0322Wt1l4WcxkAeYc5cl+3ktI4RHkhZFCODqc1wIDAQAB";

/**
 * The development Firefox add-on ID. Must equal `FIREFOX_DEV_ADDON_ID` in
 * crates/browser-host/src/lib.rs. A signed distribution needs an ID the maintainer owns (AMO
 * checks it is unique); it is added to both places when assigned.
 */
export const FIREFOX_DEV_ADDON_ID = "downova-dev@extension.downova.invalid";

/**
 * The distributed Firefox add-on ID (store builds, AMO). A UUID, so it claims no domain. Must equal
 * `FIREFOX_ADDON_ID` in crates/browser-host/src/lib.rs. Permanent once the first signed version
 * is published: Firefox treats another ID as another add-on.
 */
export const FIREFOX_ADDON_ID = "{20566e36-cebf-40ec-960b-160afd8c6ded}";

/**
 * Build flavours: `dev` (the default: fixed development identity), `e2e` (real-browser test hooks),
 * `store` (upload to the Chrome Web Store, Edge Add-ons or AMO: no `key`, because each store
 * assigns its own ID, and the distributed Firefox ID).
 */
export type Flavour = "dev" | "e2e" | "store";

const shared = {
  manifest_version: 3,
  name: "Downova",
  version: "0.1.0",
  permissions: ["contextMenus", "nativeMessaging", "storage"],
  // BRW-10: never in private windows until an end-to-end ephemeral policy exists.
  incognito: "not_allowed",
  // The desktop app's icon (apps/desktop/src-tauri/icons); stores require a 128 px icon.
  icons: { "32": "icons/icon-32.png", "64": "icons/icon-64.png", "128": "icons/icon-128.png" },
  action: {
    default_title: "Downova",
    default_popup: "popup.html",
    default_icon: { "32": "icons/icon-32.png", "64": "icons/icon-64.png" },
  },
  options_ui: { page: "options.html", open_in_tab: true },
};

export function buildManifest(
  target: Target,
  flavour: Flavour | boolean = "dev",
): Record<string, unknown> {
  // Older callers pass a boolean for e2e.
  const f: Flavour = flavour === true ? "e2e" : flavour === false ? "dev" : flavour;
  const e2e = f === "e2e";
  if (target === "firefox") {
    return {
      ...shared,
      description:
        "Hand public HTTP(S) links to the Downova download manager on this computer from the context menu. The chosen link is sent only to Downova on this computer.",
      browser_specific_settings: {
        gecko: {
          id: f === "store" ? FIREFOX_ADDON_ID : FIREFOX_DEV_ADDON_ID,
          // data_collection_permissions exists from Firefox 140 (ESR).
          strict_min_version: "140.0",
          // The link a user hands off leaves the browser for the local Downova app (native
          // messaging): Mozilla counts data handled outside the add-on as transmitted.
          data_collection_permissions: { required: ["browsingActivity"] },
        },
      },
      background: { scripts: ["background.js"], type: "module" },
    };
  }
  const optional = ["downloads", "webRequest", "cookies"];
  const hosts = ["http://*/*", "https://*/*"];
  return {
    ...shared,
    description:
      "Hand links to the Downova download manager from the context menu (optionally with your sign-in for that one site), and optionally let it take over plain public downloads from sites you choose.",
    ...(f === "store" ? {} : { key: CHROMIUM_DEV_KEY }),
    minimum_chrome_version: "116",
    background: { service_worker: "background.js", type: "module" },
    ...(e2e
      ? {
          permissions: [...shared.permissions, ...optional],
          host_permissions: ["http://127.0.0.1/*", "http://localhost/*", "https://127.0.0.1/*"],
        }
      : { optional_permissions: optional, optional_host_permissions: hosts }),
  };
}
