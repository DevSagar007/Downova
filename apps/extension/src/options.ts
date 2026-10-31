// Automatic capture settings (B2.13, ADR 0019). Permissions are requested here, inside the
// user's click (the only place the browser allows it), and only what the action needs: the
// downloads and webRequest APIs when capture is turned on, and one site's origins when that site
// is added. Turning capture off or removing a site gives the permissions back. Settings are saved
// through the background worker, which owns capture.
import {
  normalizeExtension,
  normalizeMime,
  normalizeSite,
  originsFor,
  sanitizeSettings,
  type CaptureSettings,
} from "./capture/policy.js";
import type { CaptureRecord } from "./capture/captures.js";
import { CAPTURES_KEY } from "./capture/captures.js";
import {
  browserKey,
  localeOf,
  reasonKey,
  stateKey,
  translator,
  type Translate,
} from "./capture/i18n.js";
import { CAPTURE_SETTINGS_KEY, type PopupRequest } from "./messages.js";

export const CAPTURE_APIS = ["downloads", "webRequest"];

/** One activity row's visible text (never a signed query value: `display` hides it). */
export function activityText(r: CaptureRecord, t: Translate): string[] {
  const lines = [t(stateKey(r.state)), r.display];
  if (r.state === "uncertain") lines.push(t("uncertain"));
  if (r.reason) lines.push(t(reasonKey(r.reason)));
  const side = r.state === "accepted" ? t(browserKey(r.browser)) : "";
  if (side) lines.push(side);
  if (r.jobId) lines.push(t("job", { job: r.jobId }));
  return lines;
}

/** Renders the bounded activity list (exported for tests). */
export function renderActivity(
  list: HTMLElement,
  empty: HTMLElement,
  records: CaptureRecord[],
  t: Translate,
): void {
  const doc = list.ownerDocument;
  list.replaceChildren();
  empty.hidden = records.length > 0;
  for (const r of records) {
    const li = doc.createElement("li");
    const [state, url, ...rest] = activityText(r, t);
    const s = doc.createElement("div");
    s.className = "state";
    s.dataset.state = r.state;
    s.textContent = state ?? "";
    const u = doc.createElement("div");
    u.className = "url";
    u.textContent = url ?? "";
    li.append(s, u);
    for (const line of rest) {
      const p = doc.createElement("p");
      p.textContent = line;
      li.append(p);
    }
    list.append(li);
  }
}

function text(id: string, value: string): void {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}

function send(request: PopupRequest): Promise<{ ok: boolean; value?: unknown }> {
  return chrome.runtime.sendMessage(request) as Promise<{ ok: boolean; value?: unknown }>;
}

async function load(): Promise<CaptureSettings> {
  return sanitizeSettings(
    (await chrome.storage.local.get(CAPTURE_SETTINGS_KEY))[CAPTURE_SETTINGS_KEY],
  );
}

async function save(settings: CaptureSettings): Promise<CaptureSettings> {
  const reply = await send({ type: "captureSave", settings });
  return sanitizeSettings(reply.value ?? settings);
}

function start(): void {
  const t = translator(localeOf(chrome.i18n.getUILanguage()));
  document.documentElement.lang = localeOf(chrome.i18n.getUILanguage());
  document.title = t("title");
  text("title", t("title"));
  text("intro", t("intro"));
  text("enable-heading", t("enable"));
  text("enable-label", t("enable"));
  text("enable-help", t("enableHelp"));
  text("supported-heading", t("supportedTitle"));
  const supported = document.getElementById("supported");
  if (supported) {
    supported.replaceChildren(
      ...(["supported1", "supported2", "supported3", "supported4"] as const).map((k) => {
        const li = document.createElement("li");
        li.textContent = t(k);
        return li;
      }),
    );
  }
  text("sites-heading", t("sitesTitle"));
  text("sites-help", t("sitesHelp"));
  text("site-label", t("siteLabel"));
  text("site-add", t("addSite"));
  text("excluded-heading", t("excludedTitle"));
  text("excluded-help", t("excludedHelp"));
  text("excluded-label", t("siteLabel"));
  text("excluded-add", t("addExcluded"));
  text("filters-heading", t("filtersTitle"));
  text("extensions-label", t("extensionsLabel"));
  text("mime-label", t("mimeLabel"));
  text("min-size-label", t("minSizeLabel"));
  text("filters-help", t("filtersHelp"));
  text("filters-save", t("saveFilters"));
  text("activity-heading", t("activityTitle"));
  text("no-activity", t("noActivity"));
  text("clear-activity", t("clearActivity"));

  const enabled = document.getElementById("enabled") as HTMLInputElement;
  const siteInput = document.getElementById("site-input") as HTMLInputElement;
  const excludedInput = document.getElementById("excluded-input") as HTMLInputElement;
  const problem = document.getElementById("site-problem") as HTMLElement;
  const extensions = document.getElementById("extensions") as HTMLInputElement;
  const mime = document.getElementById("mime") as HTMLInputElement;
  const minSize = document.getElementById("min-size") as HTMLInputElement;
  let current: CaptureSettings = sanitizeSettings({});

  async function showPermissions(): Promise<void> {
    const granted = await chrome.permissions.contains({ permissions: CAPTURE_APIS });
    const el = document.getElementById("permissions");
    if (!el) return;
    el.dataset.state = granted ? "granted" : "missing";
    el.textContent = granted ? t("permissionsGranted") : t("permissionsMissing");
  }

  function chips(id: string, sites: string[], empty: string, onRemove: (site: string) => void) {
    const list = document.getElementById(id);
    if (!list) return;
    list.replaceChildren();
    if (sites.length === 0) {
      const li = document.createElement("li");
      li.className = "hint";
      li.textContent = empty;
      list.append(li);
      return;
    }
    for (const site of sites) {
      const li = document.createElement("li");
      const name = document.createElement("span");
      name.textContent = site;
      name.dir = "ltr";
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = "×";
      b.setAttribute("aria-label", t("removeSite", { site }));
      b.addEventListener("click", () => onRemove(site));
      li.append(name, b);
      list.append(li);
    }
  }

  function show(s: CaptureSettings): void {
    current = s;
    enabled.checked = s.enabled;
    chips("sites", s.sites, t("noSites"), (site) => void removeSite(site));
    chips(
      "excluded",
      s.excluded,
      t("noExcluded"),
      (site) =>
        void save({ ...current, excluded: current.excluded.filter((x) => x !== site) }).then(show),
    );
    extensions.value = s.extensions.join(", ");
    mime.value = s.mimeTypes.join(", ");
    minSize.value = String(Math.floor(s.minSizeBytes / 1_000_000));
  }

  async function removeSite(site: string): Promise<void> {
    show(await save({ ...current, sites: current.sites.filter((x) => x !== site) }));
    // The site's access is given back (unless another entry still needs the same origins).
    await chrome.permissions.remove({ origins: originsFor(site) }).catch(() => false);
  }

  enabled.addEventListener("change", () => {
    const want = enabled.checked;
    void (async () => {
      if (want) {
        // Inside the click: the browser shows its prompt here.
        const ok = await chrome.permissions.request({ permissions: CAPTURE_APIS });
        if (!ok) {
          enabled.checked = false;
          await showPermissions();
          return;
        }
      }
      show(await save({ ...current, enabled: want }));
      if (!want) await chrome.permissions.remove({ permissions: CAPTURE_APIS }).catch(() => false);
      await showPermissions();
    })();
  });

  document.getElementById("site-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const site = normalizeSite(siteInput.value);
    problem.hidden = true;
    if (!site) {
      problem.textContent = t("invalidSite");
      problem.hidden = false;
      return;
    }
    void (async () => {
      const ok = await chrome.permissions.request({ origins: originsFor(site) });
      if (!ok) {
        problem.textContent = t("sitePermissionDenied", { site });
        problem.hidden = false;
        return;
      }
      siteInput.value = "";
      show(await save({ ...current, sites: [...current.sites, site] }));
    })();
  });

  document.getElementById("excluded-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const site = normalizeSite(excludedInput.value);
    if (!site) return;
    excludedInput.value = "";
    void save({ ...current, excluded: [...current.excluded, site] }).then(show);
  });

  document.getElementById("filters-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const split = (v: string) =>
      v
        .split(",")
        .map((x) => x.trim())
        .filter(Boolean);
    const mb = Math.max(0, Math.floor(Number(minSize.value) || 0));
    void save({
      ...current,
      extensions: split(extensions.value)
        .map(normalizeExtension)
        .filter((x): x is string => x !== null),
      mimeTypes: split(mime.value)
        .map(normalizeMime)
        .filter((x): x is string => x !== null),
      minSizeBytes: mb * 1_000_000,
    }).then((s) => {
      show(s);
      text("filters-status", t("saved"));
    });
  });

  async function refreshActivity(): Promise<void> {
    const stored = (await chrome.storage.local.get(CAPTURES_KEY))[CAPTURES_KEY];
    const records = Array.isArray(stored) ? (stored as CaptureRecord[]) : [];
    const list = document.getElementById("activity");
    const empty = document.getElementById("no-activity");
    const clear = document.getElementById("clear-activity");
    if (list && empty) renderActivity(list, empty, records, t);
    if (clear) clear.hidden = records.length === 0;
  }
  document
    .getElementById("clear-activity")
    ?.addEventListener("click", () => void send({ type: "captureClear" }));
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && CAPTURES_KEY in changes) void refreshActivity();
  });

  void (async () => {
    show(await load());
    await showPermissions();
    await refreshActivity();
    const service = document.getElementById("service");
    if (service) {
      service.textContent = t("serviceChecking");
      const reply = (await send({ type: "hello" })) as {
        ok: boolean;
        value?: { state: string };
      };
      const ready = reply.ok && reply.value?.state === "ready";
      service.dataset.state = ready ? "ready" : "unavailable";
      service.textContent = ready ? t("serviceReady") : t("serviceDown");
    }
  })();
}

if (typeof chrome !== "undefined" && chrome.storage && typeof document !== "undefined") start();
