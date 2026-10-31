// Status surface: the stored hand-offs with a text state (colour is never the only cue) and
// retry/dismiss actions, in the browser's language (B2.20, English/Bangla from the one extension
// catalogue). Retry goes through the background worker so it survives the popup closing, and
// reuses the hand-off's idempotency key. Rendering never sends anything.
import {
  canRetry,
  codeOf,
  displayUrl,
  settleStale,
  type HandoffRecord,
  type ServiceStatus,
} from "./handoff.js";
import { CAPTURE_SETTINGS_KEY, STORAGE_KEY, type PopupRequest } from "./messages.js";
import { sanitizeSettings } from "./capture/policy.js";
import {
  handoffCodeKey,
  handoffStateKey,
  localeOf,
  serviceKey,
  translator,
  type MessageKey,
  type Translate,
} from "./capture/i18n.js";

const english = translator("en");

/** Renders `records` into `list`; `act` is called for retry/dismiss. Exported for tests. */
export function render(
  list: HTMLElement,
  empty: HTMLElement,
  records: HandoffRecord[],
  act: (request: PopupRequest) => void,
  t: Translate = english,
): void {
  const doc = list.ownerDocument;
  list.replaceChildren();
  empty.hidden = records.length > 0;
  for (const r of records) {
    const item = doc.createElement("li");
    const state = doc.createElement("div");
    state.className = "state";
    state.dataset.state = r.state;
    state.textContent = t(handoffStateKey(r.state));
    const url = doc.createElement("div");
    url.className = "url";
    url.textContent = displayUrl(r.url);
    const detail = doc.createElement("p");
    detail.textContent = t(handoffCodeKey(codeOf(r)));
    item.append(state, url, detail);
    if (r.jobId) {
      const job = doc.createElement("div");
      job.className = "url";
      job.textContent = t("job", { job: r.jobId });
      item.append(job);
    }
    if (r.state !== "sending") {
      const actions = doc.createElement("div");
      actions.className = "actions";
      if (canRetry(r)) {
        actions.append(
          button(
            doc,
            t("handoff.retry"),
            t("handoff.retryLabel", { link: displayUrl(r.url) }),
            () => act({ type: "retry", id: r.id }),
          ),
        );
      }
      actions.append(
        button(
          doc,
          t("handoff.dismiss"),
          t("handoff.dismissLabel", { link: displayUrl(r.url) }),
          () => act({ type: "dismiss", id: r.id }),
        ),
      );
      item.append(actions);
    }
    list.append(item);
  }
}

/** Shows the readiness check result. Exported for tests. */
export function renderService(
  el: HTMLElement,
  status: ServiceStatus | { state: "checking" },
  t: Translate = english,
): void {
  el.dataset.state = status.state;
  el.textContent =
    status.state === "checking"
      ? t(serviceKey("checking"))
      : t(serviceKey(status.code), {
          version: status.version ?? "",
          backend: status.backend ?? "",
        });
}

/** Static page text, from `data-i18n` attributes. Exported for tests. */
export function localizePage(doc: Document, t: Translate, lang: string): void {
  doc.documentElement.lang = lang;
  for (const el of doc.querySelectorAll<HTMLElement>("[data-i18n]")) {
    el.textContent = t(el.dataset.i18n as MessageKey);
  }
}

function button(
  doc: Document,
  text: string,
  label: string,
  onClick: () => void,
): HTMLButtonElement {
  const b = doc.createElement("button");
  b.type = "button";
  b.textContent = text;
  b.setAttribute("aria-label", label);
  b.addEventListener("click", onClick);
  return b;
}

async function refresh(t: Translate): Promise<void> {
  const list = document.getElementById("handoffs");
  const empty = document.getElementById("empty");
  if (!list || !empty) return;
  const stored = (await chrome.storage.local.get(STORAGE_KEY))[STORAGE_KEY];
  const records = settleStale(Array.isArray(stored) ? (stored as HandoffRecord[]) : [], Date.now());
  empty.textContent = t("popup.empty");
  render(list, empty, records, (request) => void chrome.runtime.sendMessage(request), t);
  const clear = document.getElementById("clear");
  if (clear) clear.hidden = !records.some((r) => r.state !== "sending");
}

async function checkService(t: Translate): Promise<void> {
  const el = document.getElementById("service");
  if (!el) return;
  renderService(el, { state: "checking" }, t);
  const reply = (await chrome.runtime.sendMessage({ type: "hello" })) as {
    ok: boolean;
    value?: ServiceStatus;
  };
  renderService(
    el,
    reply.ok && reply.value?.code
      ? reply.value
      : { state: "uncertain", code: "no_answer", detail: "" },
    t,
  );
}

/** The capture summary and the way to its settings (B2.13). */
async function showCapture(t: Translate): Promise<void> {
  const s = sanitizeSettings(
    (await chrome.storage.local.get(CAPTURE_SETTINGS_KEY))[CAPTURE_SETTINGS_KEY],
  );
  const summary = document.getElementById("capture-summary");
  const heading = document.getElementById("capture-heading");
  const button = document.getElementById("capture-settings");
  if (heading) heading.textContent = t("popup.captureHeading");
  if (summary)
    summary.textContent =
      s.enabled && s.sites.length > 0 ? t("popupOn", { count: s.sites.length }) : t("popupOff");
  if (button) button.textContent = t("openSettings");
}

if (typeof chrome !== "undefined" && chrome.storage) {
  const locale = localeOf(chrome.i18n.getUILanguage());
  const t = translator(locale);
  localizePage(document, t, locale);
  document
    .getElementById("capture-settings")
    ?.addEventListener("click", () => void chrome.runtime.openOptionsPage());
  void showCapture(t);
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && STORAGE_KEY in changes) void refresh(t);
  });
  document.getElementById("check")?.addEventListener("click", () => void checkService(t));
  document
    .getElementById("clear")
    ?.addEventListener("click", () => void chrome.runtime.sendMessage({ type: "clear" }));
  void refresh(t);
  void checkService(t);
}
