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

const MENU_ID = "downova-download-link";

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
  void chrome.contextMenus.removeAll().then(() =>
    chrome.contextMenus.create({
      id: MENU_ID,
      title: translator(localeOf(chrome.i18n.getUILanguage()))("menuTitle"),
      contexts: ["link"],
      targetUrlPatterns: ["http://*/*", "https://*/*"],
    }),
  );
}

chrome.runtime.onInstalled.addListener(createMenu);
// Settles hand-offs interrupted by a stopped worker.
chrome.runtime.onStartup.addListener(() => void handoffs.list());

function onMenuClick(info: { menuItemId: string | number; linkUrl?: string }): void {
  if (info.menuItemId !== MENU_ID || !info.linkUrl) return;
  void handoffs.start(info.linkUrl).then(show);
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
  }
  done.then(
    (value) => respond({ ok: true, value }),
    () => respond({ ok: false }),
  );
  return true;
});
