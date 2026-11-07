// MV3 service worker: the "Download with Downova" context-menu item, the native round trip and
// (B2.13, ADR 0019) opt-in automatic capture. Nothing that matters lives only in memory: every
// hand-off and capture is in storage before its message is sent. Capture listeners are registered
// synchronously at worker start when their optional permissions are granted.
import type { NativeHostRequest } from "@dm/contracts";
import {
  HOST_NAME,
  Handoffs,
  randomToken,
  type HandoffRecord,
  type SendResult,
} from "./handoff.js";
import { CAPTURE_SETTINGS_KEY, EVIDENCE_KEY, STORAGE_KEY, isPopupRequest } from "./messages.js";
import {
  CAPTURES_KEY,
  Captures,
  type BrowserItem,
  type CaptureRecord,
} from "./capture/captures.js";
import {
  onBeforeRedirect,
  onHeadersReceived,
  onSendHeaders,
  prune,
  type Evidence,
} from "./capture/evidence.js";
import { hostMatches, sanitizeSettings, type CaptureSettings } from "./capture/policy.js";
import { localeOf, translator } from "./capture/i18n.js";
import { RECONNECTS_KEY, Reconnects, type ReconnectRecord } from "./reconnect.js";
import {
  browserKind,
  checkPage,
  checkScope,
  hostCookies,
  type ClickContext,
  httpsOrigin,
  isProfileId,
  selectCookies,
  session,
  storeForTab,
  type SignInContext,
  type SignInRefusal,
} from "./signin.js";

const MENU_ID = "downova-download-link";
// BRW-05 (ADR 0027): the signed-in hand-off, with the remember choice made in the menu itself.
const MENU_SIGNED_IN = "downova-download-signed-in";
const MENU_SIGNED_IN_REMEMBER = "downova-download-signed-in-remember";
// ADR 0028: reconnect an existing signed-in download (on an https page, not a link).
const MENU_RECONNECT = "downova-reconnect-signed-in";
/** A random id for this browser profile (not secret; keeps sign-ins of profiles apart). */
const PROFILE_KEY = "signInProfile";

async function profileId(): Promise<string> {
  const stored = (await chrome.storage.local.get(PROFILE_KEY))[PROFILE_KEY];
  if (isProfileId(stored)) return stored;
  const fresh = randomToken(16);
  await chrome.storage.local.set({ [PROFILE_KEY]: fresh });
  return fresh;
}

const originPattern = (origin: string) => `${origin}/*`;

/**
 * The session for one send: the browser's cookies for exactly this link and cookie store, read
 * now, never stored. Permission, store and origin are checked again (a retry may come later).
 */
async function sessionFor(
  context: SignInContext,
  url: string,
): Promise<{ ok: true; session: ReturnType<typeof session> } | { ok: false; code: SignInRefusal }> {
  if (httpsOrigin(url) !== context.origin)
    return { ok: false, code: "rejected_signin_cross_origin" };
  const allowed = await chrome.permissions
    .contains({ permissions: ["cookies"], origins: [originPattern(context.origin)] })
    .catch(() => false);
  if (!allowed || !chrome.cookies) return { ok: false, code: "rejected_signin_permission" };
  const stores = await chrome.cookies.getAllCookieStores();
  if (!stores.some((s) => s.id === context.storeId))
    return { ok: false, code: "rejected_signin_store" };
  const cookies = selectCookies(
    await chrome.cookies.getAll({ url, storeId: context.storeId }),
    context.storeId,
    Date.now(),
  );
  if (!cookies) return { ok: false, code: "rejected_signin_no_cookies" };
  return { ok: true, session: session(context, await profileId(), cookies) };
}

function send(message: NativeHostRequest, timeoutMs: number): Promise<SendResult> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve({ kind: "timeout" }), timeoutMs);
    chrome.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      clearTimeout(timer);
      const error = chrome.runtime.lastError;
      resolve(
        error
          ? { kind: "error", message: error.message ?? "native messaging failed" }
          : { kind: "response", value: response },
      );
    });
  });
}

const handoffs = new Handoffs({
  load: async () => {
    const stored = (await chrome.storage.local.get(STORAGE_KEY))[STORAGE_KEY];
    return Array.isArray(stored) ? (stored as HandoffRecord[]) : [];
  },
  save: (records) => chrome.storage.local.set({ [STORAGE_KEY]: records }),
  send,
  now: () => Date.now(),
  token: () => randomToken(),
  session: sessionFor,
});

/**
 * ADR 0028: the browser's current sign-in for a reconnect: the cookies of the origin's host in
 * the record's cookie store, read now (permission and store checked again), sent once.
 */
async function reconnectSession(
  record: ReconnectRecord,
): Promise<{ ok: true; session: ReturnType<typeof session> } | { ok: false; code: SignInRefusal }> {
  const allowed = await chrome.permissions
    .contains({ permissions: ["cookies"], origins: [originPattern(record.origin)] })
    .catch(() => false);
  if (!allowed || !chrome.cookies) return { ok: false, code: "rejected_signin_permission" };
  const stores = await chrome.cookies.getAllCookieStores();
  if (!stores.some((s) => s.id === record.storeId))
    return { ok: false, code: "rejected_signin_store" };
  const host = new URL(record.origin).hostname;
  const lists = await Promise.all([
    chrome.cookies.getAll({ url: `${record.origin}/`, storeId: record.storeId }),
    chrome.cookies.getAll({ domain: host, storeId: record.storeId }),
  ]);
  const cookies = selectCookies(hostCookies(record.origin, lists), record.storeId, Date.now());
  if (!cookies) return { ok: false, code: "rejected_signin_no_cookies" };
  const context: SignInContext = {
    origin: record.origin,
    storeId: record.storeId,
    browser: record.browser,
    remember: false,
  };
  return { ok: true, session: session(context, await profileId(), cookies) };
}

const reconnects = new Reconnects({
  load: async () => {
    const stored = (await chrome.storage.session.get(RECONNECTS_KEY))[RECONNECTS_KEY];
    return Array.isArray(stored) ? (stored as ReconnectRecord[]) : [];
  },
  save: (records) => chrome.storage.session.set({ [RECONNECTS_KEY]: records }),
  send,
  session: reconnectSession,
  profileId,
  now: () => Date.now(),
  token: () => randomToken(),
});

// ------------------------------------------------------------------ automatic capture (B2.13)

const CAPTURE_APIS = ["downloads", "webRequest"];

async function captureSettings(): Promise<CaptureSettings> {
  return sanitizeSettings(
    (await chrome.storage.local.get(CAPTURE_SETTINGS_KEY))[CAPTURE_SETTINGS_KEY],
  );
}

// Request evidence: write-through to storage.session, serialized, so a restarted worker sees it.
let evidenceCache: Evidence[] | null = null;
let evidenceQueue: Promise<unknown> = Promise.resolve();
function updateEvidence(f: (list: Evidence[]) => Evidence[]): void {
  evidenceQueue = evidenceQueue
    .then(async () => {
      if (!evidenceCache) {
        const stored = (await chrome.storage.session.get(EVIDENCE_KEY))[EVIDENCE_KEY];
        evidenceCache = Array.isArray(stored) ? (stored as Evidence[]) : [];
      }
      evidenceCache = prune(f(evidenceCache), Date.now());
      await chrome.storage.session.set({ [EVIDENCE_KEY]: evidenceCache });
    })
    .catch(() => undefined);
}
async function currentEvidence(): Promise<Evidence[]> {
  updateEvidence((l) => l);
  await evidenceQueue;
  return evidenceCache ?? [];
}

function toItem(d: chrome.DownloadItem): BrowserItem {
  return {
    id: d.id,
    url: d.url,
    finalUrl: d.finalUrl || d.url,
    mime: d.mime ?? "",
    totalBytes: d.totalBytes,
    filename: d.filename ?? "",
    danger: d.danger,
    incognito: d.incognito,
    byExtensionId: d.byExtensionId,
    state: d.state,
    startTime: d.startTime,
    error: d.error,
  };
}

/** Host → whether a granted origin pattern covers it (host part only; both schemes are asked). */
async function hostPermissions(): Promise<(host: string) => boolean> {
  const origins = (await chrome.permissions.getAll()).origins ?? [];
  const hosts = origins
    .map((o) => /^[a-z*]+:\/\/([^/]+)\//.exec(o)?.[1])
    .filter((h): h is string => typeof h === "string");
  return (host) =>
    hosts.some((h) =>
      h === "*" ? true : h.startsWith("*.") ? hostMatches(host, h.slice(2)) : h === host,
    );
}

const captures = new Captures({
  load: async () => {
    const stored = (await chrome.storage.local.get(CAPTURES_KEY))[CAPTURES_KEY];
    return Array.isArray(stored) ? (stored as CaptureRecord[]) : [];
  },
  save: (records) => chrome.storage.local.set({ [CAPTURES_KEY]: records }),
  settings: captureSettings,
  hasApis: () => chrome.permissions.contains({ permissions: CAPTURE_APIS }),
  hostPermissions,
  evidence: currentEvidence,
  item: async (id) => {
    if (!chrome.downloads) return null;
    const [found] = await chrome.downloads.search({ id });
    return found ? toItem(found) : null;
  },
  // Only ever after a matching acceptance (captures.ts). Never erase or removeFile.
  cancel: async (id) => {
    await chrome.downloads?.cancel(id);
  },
  send,
  now: () => Date.now(),
  token: () => randomToken(),
  sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
});

// Only requests that can become downloads; header NAMES are read, values never stored.
const REQUEST_FILTER = {
  urls: ["http://*/*", "https://*/*"],
  types: ["main_frame", "sub_frame", "other"],
};
let captureListeners = false;

/** Registers the capture listeners once their APIs exist (synchronously at worker start). */
function registerCapture(): void {
  if (captureListeners || !chrome.downloads || !chrome.webRequest) return;
  captureListeners = true;
  chrome.webRequest.onSendHeaders.addListener(
    (d) => updateEvidence((l) => onSendHeaders(l, d, Date.now())),
    REQUEST_FILTER,
    ["requestHeaders", "extraHeaders"],
  );
  chrome.webRequest.onBeforeRedirect.addListener(
    (d) =>
      updateEvidence((l) =>
        d.redirectUrl
          ? onBeforeRedirect(l, { requestId: d.requestId, redirectUrl: d.redirectUrl })
          : l,
      ),
    REQUEST_FILTER,
  );
  chrome.webRequest.onHeadersReceived.addListener(
    (d) =>
      updateEvidence((l) =>
        onHeadersReceived(
          l,
          { requestId: d.requestId, url: d.url, statusCode: d.statusCode ?? 0 },
          Date.now(),
        ),
      ),
    REQUEST_FILTER,
  );
  chrome.downloads.onCreated.addListener(
    (d) => void captures.onCreated(toItem(d)).then(showCapture),
  );
  chrome.downloads.onChanged.addListener(
    (delta) =>
      void captures.onChanged({
        id: delta.id,
        state: delta.state?.current,
        error: delta.error?.current,
      }),
  );
}
registerCapture();
chrome.permissions.onAdded.addListener(() => registerCapture());
chrome.permissions.onRemoved.addListener((removed) => {
  if (
    (removed.permissions ?? []).some((p) => CAPTURE_APIS.includes(p)) ||
    removed.origins?.length
  ) {
    void captures.abandonInFlight("permission_revoked");
  }
});
// Captures interrupted by a stopped worker continue with their stored keys.
void captures.resume();

async function showCapture(record: CaptureRecord | null): Promise<void> {
  if (!record || record.state === "browser_owned") return;
  const state =
    record.state === "accepted" ? "accepted" : record.state === "uncertain" ? "uncertain" : null;
  if (!state) return;
  const [text, color] = BADGE[state];
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setBadgeText({ text });
}

const BADGE: Record<HandoffRecord["state"], [string, string]> = {
  sending: ["…", "#5b6472"],
  accepted: ["✓", "#1f7a4d"],
  rejected: ["!", "#b42318"],
  unavailable: ["!", "#b42318"],
  uncertain: ["?", "#b54708"],
};

async function show(record: HandoffRecord | null): Promise<void> {
  if (!record) return;
  const [text, color] = BADGE[record.state];
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setBadgeText({ text });
}

function createMenu(): void {
  const t = translator(localeOf(chrome.i18n.getUILanguage()));
  void chrome.contextMenus.removeAll().then(() => {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: t("menuTitle"),
      contexts: ["link"],
      targetUrlPatterns: ["http://*/*", "https://*/*"],
    });
    // Signed-in hand-offs exist only for https links.
    for (const [id, key] of [
      [MENU_SIGNED_IN, "menuSignedIn"],
      [MENU_SIGNED_IN_REMEMBER, "menuSignedInRemember"],
    ] as const)
      chrome.contextMenus.create({
        id,
        title: t(key),
        contexts: ["link"],
        targetUrlPatterns: ["https://*/*"],
      });
    chrome.contextMenus.create({
      id: MENU_RECONNECT,
      title: t("menuReconnect"),
      contexts: ["page", "frame"],
      documentUrlPatterns: ["https://*/*"],
    });
  });
}

chrome.runtime.onInstalled.addListener(createMenu);
// Settles hand-offs interrupted by a stopped worker.
chrome.runtime.onStartup.addListener(() => void handoffs.list());

function onMenuClick(info: chrome.MenuInfo, tab?: chrome.Tab): void {
  if (info.menuItemId === MENU_RECONNECT) {
    reconnectFrom({ pageUrl: info.pageUrl, frameUrl: info.frameUrl }, tab);
    return;
  }
  if (!info.linkUrl) return;
  if (info.menuItemId === MENU_ID) {
    void handoffs.start(info.linkUrl).then(show);
    return;
  }
  if (info.menuItemId !== MENU_SIGNED_IN && info.menuItemId !== MENU_SIGNED_IN_REMEMBER) return;
  void signedIn(
    info.linkUrl,
    { pageUrl: info.pageUrl, frameUrl: info.frameUrl },
    tab,
    info.menuItemId === MENU_SIGNED_IN_REMEMBER,
  );
}

/**
 * ADR 0028: "Reconnect a Downova download with my sign-in". The page rule first; the permission
 * prompt opens inside the click; then Downova lists this origin's downloads for this browser
 * profile, and the reconnect page asks the user to choose one.
 */
function reconnectFrom(where: ClickContext, tab: chrome.Tab | undefined): void {
  const page = checkPage(where, tab?.incognito ?? false);
  const open = (record: ReconnectRecord) =>
    chrome.tabs.create({ url: chrome.runtime.getURL(`reconnect.html?id=${record.id}`) });
  if (!page.ok) {
    const origin = where.pageUrl ? (httpsOrigin(where.pageUrl) ?? "") : "";
    void reconnects.refuse(origin, page.code).then(open);
    return;
  }
  const wanted = { permissions: ["cookies"], origins: [originPattern(page.origin)] };
  const asked = chrome.permissions
    .request(wanted)
    .catch(() => chrome.permissions.contains(wanted).catch(() => false));
  void (async () => {
    if (!(await asked) || !chrome.cookies)
      return open(await reconnects.refuse(page.origin, "rejected_signin_permission"));
    const storeId = storeForTab(await chrome.cookies.getAllCookieStores(), tab?.id);
    if (!storeId) return open(await reconnects.refuse(page.origin, "rejected_signin_store"));
    // The page opens at once and shows "asking Downova…" until the list arrives.
    await reconnects.begin(page.origin, storeId, browserKind(navigator.userAgent), undefined, open);
  })();
}

/**
 * BRW-05: one user action. The scope is checked first; the permission prompt (cookies plus this
 * one https origin) opens inside the click, before anything is awaited, as the browser requires.
 */
function signedIn(
  link: string,
  where: ClickContext,
  tab: chrome.Tab | undefined,
  remember: boolean,
): Promise<unknown> {
  const scope = checkScope(link, where, tab?.incognito ?? false);
  if (!scope.ok) return handoffs.start(link, undefined, scope.code).then(show);
  const wanted = { permissions: ["cookies"], origins: [originPattern(scope.origin)] };
  // Without a user gesture (automation) the prompt cannot open; an existing grant still counts.
  const asked = chrome.permissions
    .request(wanted)
    .catch(() => chrome.permissions.contains(wanted).catch(() => false));
  return (async () => {
    if (!(await asked) || !chrome.cookies) {
      return show(await handoffs.start(link, undefined, "rejected_signin_permission"));
    }
    const storeId = storeForTab(await chrome.cookies.getAllCookieStores(), tab?.id);
    if (!storeId) return show(await handoffs.start(link, undefined, "rejected_signin_store"));
    const context: SignInContext = {
      origin: scope.origin,
      storeId,
      browser: browserKind(navigator.userAgent),
      remember,
    };
    return show(await handoffs.start(link, undefined, context));
  })();
}
chrome.contextMenus.onClicked.addListener(onMenuClick);

// Real-browser end-to-end tests only (`vite build --mode e2e`; absent from normal builds): Chrome
// offers no way to click its native context menu from automation, so the test calls the same
// listener with the same input, and can shorten the acknowledgement timeout to lose an ack.
if (import.meta.env.MODE === "e2e") {
  Object.assign(globalThis, {
    downovaE2E: {
      menuClick: onMenuClick,
      menuId: MENU_ID,
      start: (url: string, timeoutMs?: number) => handoffs.start(url, timeoutMs),
      // BRW-05: the signed-in path after the menu click (the permission must already be granted
      // by the test, since automation cannot answer the browser's prompt).
      // The tab defaults to the first tab of the default cookie store (the test browser's own).
      signedIn: async (link: string, frameUrl: string, remember: boolean, tabId?: number) => {
        const id =
          tabId ??
          (await chrome.cookies?.getAllCookieStores())?.find((s) => s.id === "0")?.tabIds[0];
        // Awaited, so a failure surfaces in the test instead of leaving no record.
        await signedIn(
          link,
          { pageUrl: frameUrl },
          { ...(id === undefined ? {} : { id }), incognito: false },
          remember,
        );
      },
      // ADR 0028: the reconnect path after its menu click, then the page's explicit choice.
      reconnectBegin: async (pageUrl: string, frameUrl?: string) => {
        const where: ClickContext = { pageUrl, ...(frameUrl ? { frameUrl } : {}) };
        const page = checkPage(where, false);
        if (!page.ok) return reconnects.refuse(pageUrl, page.code);
        const id = (await chrome.cookies?.getAllCookieStores())?.find((s) => s.id === "0")
          ?.tabIds[0];
        const storeId = storeForTab((await chrome.cookies?.getAllCookieStores()) ?? [], id);
        if (!storeId) return reconnects.refuse(page.origin, "rejected_signin_store");
        return reconnects.begin(page.origin, storeId, browserKind(navigator.userAgent));
      },
      reconnectSubmit: (id: string, jobId: string, resume: boolean, timeoutMs?: number) =>
        reconnects.submit(id, jobId, resume, timeoutMs),
      reconnectMenuId: MENU_RECONNECT,
      sessionStorage: () => chrome.storage.session.get(null as unknown as string),
      // Everything this extension keeps (to prove no cookie is ever stored).
      storage: () => chrome.storage.local.get(null as unknown as string),
      menuIds: [MENU_ID, MENU_SIGNED_IN, MENU_SIGNED_IN_REMEMBER],
      retry: (id: string) => handoffs.retry(id),
      list: () => handoffs.list(),
      // Capture (B2.13): the real downloads/webRequest events drive it; tests read the outcome
      // and set the settings the options page would save after its permission prompts.
      captures: () => captures.list(),
      evidence: () => currentEvidence(),
      setCaptureSettings: (s: unknown) =>
        chrome.storage.local.set({ [CAPTURE_SETTINGS_KEY]: sanitizeSettings(s) }),
      captureListeners: () => captureListeners,
      // The browser's own view of its downloads (state only; read, never changed here).
      downloads: async () =>
        ((await chrome.downloads?.search({} as { id: number })) ?? []).map((d) => ({
          id: d.id,
          url: d.url,
          state: d.state,
          error: d.error ?? null,
        })),
      replayCreated: async (id: number) => {
        const [d] = (await chrome.downloads?.search({ id })) ?? [];
        return d ? captures.onCreated(toItem(d)) : null;
      },
    },
  });
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || !isPopupRequest(message)) return false;
  let done: Promise<unknown>;
  switch (message.type) {
    case "retry":
      done = handoffs.retry(message.id).then(show);
      break;
    case "dismiss":
      done = handoffs.dismiss(message.id);
      break;
    case "clear":
      done = handoffs.clear().then(() => chrome.action.setBadgeText({ text: "" }));
      break;
    case "hello":
      done = handoffs.hello();
      break;
    case "captureSave":
      done = (async () => {
        const next = sanitizeSettings(message.settings);
        const previous = await captureSettings();
        await chrome.storage.local.set({ [CAPTURE_SETTINGS_KEY]: next });
        if (previous.enabled && !next.enabled) await captures.abandonInFlight("capture_disabled");
        registerCapture();
        return next;
      })();
      break;
    case "captureClear":
      done = captures.clear();
      break;
    case "reconnectGet":
      done = reconnects.get(message.id);
      break;
    case "reconnectSubmit":
      done = reconnects.submit(message.id, message.jobId, message.resume);
      break;
  }
  done.then(
    (value) => respond({ ok: true, value }),
    () => respond({ ok: false }),
  );
  return true;
});
