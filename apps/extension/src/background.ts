// MV3 service worker: the "Download with Downova" context-menu item and the native round trip.
// It never touches browser downloads (no downloads permission) and holds nothing that matters
// only in memory: every hand-off is in storage before its message is sent.
import type { NativeHostAdd } from "@dm/contracts";
import {
  HOST_NAME,
  Handoffs,
  randomToken,
  type HandoffRecord,
  type SendResult,
} from "./handoff.js";
import { STORAGE_KEY, isPopupRequest } from "./messages.js";

const MENU_ID = "downova-download-link";

function send(message: NativeHostAdd, timeoutMs: number): Promise<SendResult> {
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
      title: "Download with Downova",
      contexts: ["link"],
      targetUrlPatterns: ["http://*/*", "https://*/*"],
    }),
  );
}

chrome.runtime.onInstalled.addListener(createMenu);
// Settles hand-offs interrupted by a stopped worker.
chrome.runtime.onStartup.addListener(() => void handoffs.list());

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId !== MENU_ID || !info.linkUrl) return;
  void handoffs.start(info.linkUrl).then(show);
});

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || !isPopupRequest(message)) return false;
  const done =
    message.type === "retry" ? handoffs.retry(message.id).then(show) : handoffs.dismiss(message.id);
  void done.then(
    () => respond({ ok: true }),
    () => respond({ ok: false }),
  );
  return true;
});
