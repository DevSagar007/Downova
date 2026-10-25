// Status surface: the stored hand-offs with a text state (colour is never the only cue) and
// retry/dismiss actions. Retry goes through the background worker so it survives the popup
// closing, and reuses the hand-off's idempotency key.
import {
  canRetry,
  describe,
  displayUrl,
  settleStale,
  type HandoffRecord,
  type ServiceState,
} from "./handoff.js";
import { STORAGE_KEY, type PopupRequest } from "./messages.js";

const LABEL: Record<HandoffRecord["state"], string> = {
  sending: "Sending",
  accepted: "Accepted by Downova",
  rejected: "Not accepted",
  unavailable: "Downova unavailable",
  uncertain: "Not confirmed",
};

/** Renders `records` into `list`; `act` is called for retry/dismiss. Exported for tests. */
export function render(
  list: HTMLElement,
  empty: HTMLElement,
  records: HandoffRecord[],
  act: (request: PopupRequest) => void,
): void {
  const doc = list.ownerDocument;
  list.replaceChildren();
  empty.hidden = records.length > 0;
  for (const r of records) {
    const item = doc.createElement("li");
    const state = doc.createElement("div");
    state.className = "state";
    state.dataset.state = r.state;
    state.textContent = LABEL[r.state];
    const url = doc.createElement("div");
    url.className = "url";
    url.textContent = displayUrl(r.url);
    const detail = doc.createElement("p");
    detail.textContent = describe(r);
    item.append(state, url, detail);
    if (r.jobId) {
      const job = doc.createElement("div");
      job.className = "url";
      job.textContent = `Job ${r.jobId}`;
      item.append(job);
    }
    if (r.state !== "sending") {
      const actions = doc.createElement("div");
      actions.className = "actions";
      if (canRetry(r)) {
        actions.append(
          button(doc, "Retry", `Retry hand-off of ${displayUrl(r.url)}`, () =>
            act({ type: "retry", id: r.id }),
          ),
        );
      }
      actions.append(
        button(doc, "Dismiss", `Dismiss ${displayUrl(r.url)}`, () =>
          act({ type: "dismiss", id: r.id }),
        ),
      );
      item.append(actions);
    }
    list.append(item);
  }
}

const SERVICE_LABEL: Record<ServiceState | "checking", string> = {
  checking: "Checking whether Downova is running…",
  ready: "Service ready.",
  unavailable: "Service unavailable.",
  uncertain: "Service status unknown.",
};

/** Shows the readiness check result. Exported for tests. */
export function renderService(
  el: HTMLElement,
  status: { state: ServiceState | "checking"; detail?: string },
): void {
  el.dataset.state = status.state;
  el.textContent = status.detail
    ? `${SERVICE_LABEL[status.state]} ${status.detail}`
    : SERVICE_LABEL[status.state];
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

async function refresh(): Promise<void> {
  const list = document.getElementById("handoffs");
  const empty = document.getElementById("empty");
  if (!list || !empty) return;
  const stored = (await chrome.storage.local.get(STORAGE_KEY))[STORAGE_KEY];
  const records = settleStale(Array.isArray(stored) ? (stored as HandoffRecord[]) : [], Date.now());
  render(list, empty, records, (request) => void chrome.runtime.sendMessage(request));
  const clear = document.getElementById("clear");
  if (clear) clear.hidden = !records.some((r) => r.state !== "sending");
}

async function checkService(): Promise<void> {
  const el = document.getElementById("service");
  if (!el) return;
  renderService(el, { state: "checking" });
  const reply = (await chrome.runtime.sendMessage({ type: "hello" })) as {
    ok: boolean;
    value?: { state: ServiceState; detail: string };
  };
  renderService(el, reply.ok && reply.value ? reply.value : { state: "uncertain" });
}

if (typeof chrome !== "undefined" && chrome.storage) {
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && STORAGE_KEY in changes) void refresh();
  });
  document.getElementById("check")?.addEventListener("click", () => void checkService());
  document
    .getElementById("clear")
    ?.addEventListener("click", () => void chrome.runtime.sendMessage({ type: "clear" }));
  void refresh();
  void checkService();
}
