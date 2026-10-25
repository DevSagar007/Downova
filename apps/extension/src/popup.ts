// Status surface: the stored hand-offs with a text state (colour is never the only cue) and
// retry/dismiss actions. Retry goes through the background worker so it survives the popup
// closing, and reuses the hand-off's idempotency key.
import { canRetry, describe, settleStale, type HandoffRecord } from "./handoff.js";
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
    url.textContent = r.url;
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
          button(doc, "Retry", `Retry hand-off of ${r.url}`, () =>
            act({ type: "retry", id: r.id }),
          ),
        );
      }
      actions.append(
        button(doc, "Dismiss", `Dismiss ${r.url}`, () => act({ type: "dismiss", id: r.id })),
      );
      item.append(actions);
    }
    list.append(item);
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

async function refresh(): Promise<void> {
  const list = document.getElementById("handoffs");
  const empty = document.getElementById("empty");
  if (!list || !empty) return;
  const stored = (await chrome.storage.local.get(STORAGE_KEY))[STORAGE_KEY];
  const records = settleStale(Array.isArray(stored) ? (stored as HandoffRecord[]) : [], Date.now());
  render(list, empty, records, (request) => void chrome.runtime.sendMessage(request));
}

if (typeof chrome !== "undefined" && chrome.storage) {
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && STORAGE_KEY in changes) void refresh();
  });
  void refresh();
}
