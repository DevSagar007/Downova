// The reconnect page (BRW-05, ADR 0028): the downloads Downova offered for this site, one chosen
// explicitly (nothing preselected), then "Reconnect" or "Reconnect and resume" through the
// background worker (which reads the sign-in at that moment). Rendering never sends anything.
import { RECONNECTS_KEY, type ReconnectRecord } from "./reconnect.js";
import { localeOf, translator, type MessageKey, type Translate } from "./capture/i18n.js";
import { localizePage } from "./popup.js";

const english = translator("en");

function bytes(n: string | undefined): string {
  const v = Number(n ?? "0");
  if (!Number.isFinite(v)) return "?";
  if (v >= 1024 * 1024) return `${(v / (1024 * 1024)).toFixed(1)} MB`;
  if (v >= 1024) return `${Math.round(v / 1024)} KB`;
  return `${v} B`;
}

/** The status sentence for a record (a stable key, then the translated text). */
export function statusKey(r: ReconnectRecord): MessageKey {
  if (r.state === "reconnected")
    return r.phase === "failed"
      ? "reconnect.state.reconnectedFailed"
      : "reconnect.state.reconnected";
  if (r.state === "refused") {
    const known: Record<string, MessageKey> = {
      rejected_signin_insecure: "handoff.code.rejected_signin_insecure",
      rejected_signin_private: "handoff.code.rejected_signin_private",
      rejected_signin_permission: "handoff.code.rejected_signin_permission",
      rejected_signin_store: "handoff.code.rejected_signin_store",
      rejected_signin_no_cookies: "handoff.code.rejected_signin_no_cookies",
      rejected_signin_partitioned: "handoff.code.rejected_signin_partitioned",
      AUTH_SCOPE_UNSUPPORTED: "reconnect.refused.scope",
      INVALID_REQUEST: "reconnect.refused.changed",
      IDEMPOTENCY_CONFLICT: "reconnect.refused.changed",
      AUTH_REQUIRED: "reconnect.refused.noSignIn",
      LINK_REFRESH_REQUIRED: "reconnect.refused.linkLost",
    };
    return known[r.code ?? ""] ?? "reconnect.refused.other";
  }
  if (r.state === "unavailable")
    return r.code === "CREDENTIAL_STORE_UNAVAILABLE"
      ? "reconnect.state.storeUnavailable"
      : "reconnect.state.unavailable";
  return `reconnect.state.${r.state}` as MessageKey;
}

/** Renders `record`; `submit(jobId, resume)` is called on an explicit choice. Exported for tests. */
export function render(
  doc: Document,
  record: ReconnectRecord | null,
  submit: (jobId: string, resume: boolean) => void,
  t: Translate = english,
): void {
  const site = doc.getElementById("site");
  const form = doc.getElementById("choose") as HTMLFormElement | null;
  const list = doc.getElementById("candidates");
  const status = doc.getElementById("status");
  const go = doc.getElementById("reconnect") as HTMLButtonElement | null;
  const goResume = doc.getElementById("reconnect-resume") as HTMLButtonElement | null;
  if (!site || !form || !list || !status || !go || !goResume) return;
  if (!record) {
    form.hidden = true;
    status.dataset.state = "refused";
    status.textContent = t("reconnect.state.expired");
    return;
  }
  site.textContent = t("reconnect.site", { origin: record.origin });
  status.dataset.state = record.state;
  status.textContent = record.state === "choosing" ? "" : t(statusKey(record));
  const choosing = ["choosing", "uncertain", "unavailable", "refused", "sending"].includes(
    record.state,
  );
  form.hidden = record.candidates.length === 0 || !choosing;
  const previous = (form.querySelector("input[name=job]:checked") as HTMLInputElement | null)
    ?.value;
  list.replaceChildren();
  for (const c of record.candidates) {
    const label = doc.createElement("label");
    label.className = "candidate";
    const input = doc.createElement("input");
    input.type = "radio";
    input.name = "job";
    input.value = c.job_id;
    input.checked = previous === c.job_id || (previous === undefined && record.jobId === c.job_id);
    const text = doc.createElement("span");
    const name = doc.createElement("strong");
    name.textContent = c.display_name;
    const detail = doc.createElement("small");
    detail.textContent = [
      t(`reconnect.auth.${c.auth.state}` as MessageKey),
      c.desired_state === "paused" ? t("reconnect.paused") : "",
      t("reconnect.progress", {
        done: bytes(c.downloaded_bytes),
        total: c.total_bytes ? bytes(c.total_bytes) : "?",
      }),
    ]
      .filter(Boolean)
      .join(" · ");
    text.append(name, detail);
    label.append(input, text);
    list.append(label);
  }
  const busy = record.state === "sending";
  const chosen = () =>
    (form.querySelector("input[name=job]:checked") as HTMLInputElement | null)?.value;
  const sync = () => {
    const none = chosen() === undefined;
    go.disabled = busy || none;
    goResume.disabled = busy || none;
    go.setAttribute("aria-busy", String(busy));
  };
  for (const input of list.querySelectorAll("input")) {
    input.disabled = busy;
    input.addEventListener("change", sync);
  }
  sync();
  form.onsubmit = (e) => {
    e.preventDefault();
    const job = chosen();
    if (job && !go.disabled) submit(job, false);
  };
  goResume.onclick = () => {
    const job = chosen();
    if (job && !goResume.disabled) submit(job, true);
  };
}

if (typeof chrome !== "undefined" && chrome.storage) {
  const locale = localeOf(chrome.i18n.getUILanguage());
  const t = translator(locale);
  localizePage(document, t, locale);
  const id = new URLSearchParams(location.search).get("id") ?? "";
  let inFlight = false;
  const show = (record: ReconnectRecord | null) =>
    render(
      document,
      record,
      (jobId, resume) => {
        if (inFlight) return; // One request per choice, however fast the clicks come.
        inFlight = true;
        if (record) show({ ...record, state: "sending", jobId, resume });
        void chrome.runtime
          .sendMessage({ type: "reconnectSubmit", id, jobId, resume })
          .then((reply) => show((reply as { value?: ReconnectRecord }).value ?? null))
          .finally(() => (inFlight = false));
      },
      t,
    );
  const refresh = () =>
    void chrome.runtime.sendMessage({ type: "reconnectGet", id }).then((reply) => {
      if (!inFlight) show((reply as { value?: ReconnectRecord }).value ?? null);
    });
  // The list arrives after the page opened ("asking Downova…"); later answers too.
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "session" && RECONNECTS_KEY in changes) refresh();
  });
  refresh();
}
