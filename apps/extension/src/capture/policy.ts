// Automatic capture policy (B2.13, ADR 0019): settings, filters and the eligibility decision.
//
// Pure functions only. A browser download is handed to Downova only when every check passes;
// anything unknown, ambiguous or unsupported stays in the browser with a reason the user can act
// on. Filenames and URLs are data: they are parsed, never used as paths.
import { checkLink } from "../handoff.js";
import type { Evidence } from "./evidence.js";

export interface CaptureSettings {
  /** Off by default; turning it on asks for the optional permissions. */
  enabled: boolean;
  /** Sites whose downloads may be captured (host names; subdomains included). */
  sites: string[];
  /** Sites never captured, even when they match `sites` (exclusions win). */
  excluded: string[];
  /** Lower-case file extensions without the dot; empty = any (except the always-excluded types). */
  extensions: string[];
  /** MIME types (`type/subtype` or `type/*`); empty = any. */
  mimeTypes: string[];
  /** Smallest size captured; 0 = any. With a minimum, an unknown size stays in the browser. */
  minSizeBytes: number;
}

export const DEFAULT_SETTINGS: CaptureSettings = {
  enabled: false,
  sites: [],
  excluded: [],
  extensions: [],
  mimeTypes: [],
  minSizeBytes: 0,
};

export const MAX_SITES = 50;
export const MAX_FILTERS = 50;

/**
 * Executable and installer types always stay in the browser, whatever the filters say: the
 * browser's own safety checks (dangerous-file warnings, Safe Browsing) must keep applying to them.
 */
export const ALWAYS_BROWSER_EXTENSIONS: readonly string[] = [
  "exe",
  "msi",
  "msix",
  "msixbundle",
  "appx",
  "bat",
  "cmd",
  "com",
  "scr",
  "pif",
  "cpl",
  "ps1",
  "psm1",
  "vbs",
  "vbe",
  "js",
  "jse",
  "wsf",
  "wsh",
  "hta",
  "lnk",
  "reg",
  "jar",
  "dmg",
  "pkg",
  "app",
  "command",
  "sh",
  "run",
  "deb",
  "rpm",
  "appimage",
  "apk",
  "xpi",
  "crx",
];

/** Why a download stays in the browser (shown in the activity list, localized). */
export type Reason =
  | "unsupported_scheme"
  | "invalid_link"
  | "incognito"
  | "site_not_allowed"
  | "site_excluded"
  | "permission_missing"
  | "browser_warning"
  | "executable"
  | "no_request_evidence"
  | "ambiguous_request"
  | "not_get"
  | "sent_cookies"
  | "sent_credentials"
  | "unexpected_status"
  | "redirect_not_allowed"
  | "mime_unknown"
  | "mime_filtered"
  | "type_filtered"
  | "size_unknown"
  | "too_small"
  | "busy"
  | "capture_disabled"
  | "permission_revoked"
  | "downova_unavailable"
  | "downova_rejected";

/** The browser's view of one download, as far as the policy needs it. */
export interface Item {
  id: number;
  url: string;
  finalUrl: string;
  mime: string;
  totalBytes: number;
  filename: string;
  danger: string;
  incognito: boolean;
  byExtensionId?: string | undefined;
  state: string;
}

export type Decision = { eligible: true } | { eligible: false; reason: Reason };

// ------------------------------------------------------------------ site matching

/** A site entry as a bare lower-case host name (scheme, port, path and a leading `*.` dropped). */
export function normalizeSite(raw: string): string | null {
  let s = raw.trim().toLowerCase();
  if (s === "") return null;
  s = s.replace(/^[a-z][a-z0-9+.-]*:\/\//, "").replace(/^\*\./, "");
  s = s.split(/[/?#]/)[0] ?? "";
  // IPv6 literals are not supported as sites; a trailing port is ignored.
  if (s.startsWith("[")) return null;
  s = s.replace(/:\d+$/, "").replace(/\.$/, "");
  if (!/^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(s) || s.length > 253) return null;
  if (s.split(".").some((label) => label.length === 0 || label.length > 63)) return null;
  return s;
}

/** True when `host` is `site` or one of its subdomains (a dot boundary, never a suffix match). */
export function hostMatches(host: string, site: string): boolean {
  const h = host.toLowerCase().replace(/\.$/, "");
  return h === site || h.endsWith(`.${site}`);
}

export function siteVerdict(
  host: string,
  settings: Pick<CaptureSettings, "sites" | "excluded">,
): "allowed" | "excluded" | "not_allowed" {
  if (settings.excluded.some((s) => hostMatches(host, s))) return "excluded";
  return settings.sites.some((s) => hostMatches(host, s)) ? "allowed" : "not_allowed";
}

/** The host-permission patterns a site needs (asked for when the site is added). */
export function originsFor(site: string): string[] {
  const exact = [`http://${site}/*`, `https://${site}/*`];
  // A numeric address has no subdomains.
  return /^[0-9.]+$/.test(site) ? exact : [...exact, `http://*.${site}/*`, `https://*.${site}/*`];
}

// ------------------------------------------------------------------ filters

/** The last extension of a file name, treated as data (path separators end the name). */
export function extensionOf(filename: string): string {
  const base = filename.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

export function normalizeExtension(raw: string): string | null {
  const e = raw
    .trim()
    .toLowerCase()
    .replace(/^\*?\./, "");
  return /^[a-z0-9]{1,10}$/.test(e) ? e : null;
}

export function normalizeMime(raw: string): string | null {
  const m = raw.trim().toLowerCase();
  return /^[a-z0-9][a-z0-9!#$&^_.+-]*\/(\*|[a-z0-9][a-z0-9!#$&^_.+-]*)$/.test(m) ? m : null;
}

function mimeMatches(mime: string, filter: string): boolean {
  const m = mime.split(";")[0]?.trim().toLowerCase() ?? "";
  return filter.endsWith("/*") ? m.startsWith(filter.slice(0, -1)) : m === filter;
}

/** Normalizes stored or user-entered settings; invalid entries are dropped, lists bounded. */
export function sanitizeSettings(raw: unknown): CaptureSettings {
  const r = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
  const list = (v: unknown, norm: (s: string) => string | null, max: number) =>
    Array.isArray(v)
      ? [
          ...new Set(
            v
              .filter((x): x is string => typeof x === "string")
              .map(norm)
              .filter((x): x is string => x !== null),
          ),
        ].slice(0, max)
      : [];
  const min = typeof r.minSizeBytes === "number" ? Math.floor(r.minSizeBytes) : 0;
  return {
    enabled: r.enabled === true,
    sites: list(r.sites, normalizeSite, MAX_SITES),
    excluded: list(r.excluded, normalizeSite, MAX_SITES),
    extensions: list(r.extensions, normalizeExtension, MAX_FILTERS),
    mimeTypes: list(r.mimeTypes, normalizeMime, MAX_FILTERS),
    minSizeBytes: Number.isFinite(min) && min > 0 ? Math.min(min, Number.MAX_SAFE_INTEGER) : 0,
  };
}

// ------------------------------------------------------------------ the decision

function hostOf(raw: string): string | null {
  try {
    return new URL(raw).hostname;
  } catch {
    return null;
  }
}

/**
 * Whether a browser download may be handed to Downova. Precedence (first failing check wins):
 * scheme and link → incognito → sites (exclusions first) for the start and final URL → host
 * permission → the browser's danger verdict → always-browser types → request evidence (one GET,
 * no cookies or credentials, 200, every redirect hop allowed) → MIME, extension and size filters.
 */
export function decide(
  item: Item,
  evidence: { kind: "match"; evidence: Evidence } | { kind: "none" } | { kind: "ambiguous" },
  settings: CaptureSettings,
  hasHostPermission: (host: string) => boolean,
): Decision {
  const no = (reason: Reason): Decision => ({ eligible: false, reason });
  for (const u of [item.url, item.finalUrl]) {
    if (!/^https?:/i.test(u)) return no("unsupported_scheme");
    if (!checkLink(u).ok) return no("invalid_link");
  }
  if (item.incognito) return no("incognito");
  const hosts = [hostOf(item.url), hostOf(item.finalUrl)];
  for (const host of hosts) {
    if (host === null) return no("invalid_link");
    const verdict = siteVerdict(host, settings);
    if (verdict === "excluded") return no("site_excluded");
    if (verdict === "not_allowed") return no("site_not_allowed");
    if (!hasHostPermission(host)) return no("permission_missing");
  }
  if (item.danger !== "safe") return no("browser_warning");
  const ext = extensionOf(item.filename);
  if (ALWAYS_BROWSER_EXTENSIONS.includes(ext)) return no("executable");

  if (evidence.kind === "none") return no("no_request_evidence");
  if (evidence.kind === "ambiguous") return no("ambiguous_request");
  const e = evidence.evidence;
  if (e.method !== "GET") return no("not_get");
  if (e.sentCookies) return no("sent_cookies");
  if (e.sentCredentials) return no("sent_credentials");
  if (e.status !== 200) return no("unexpected_status");
  for (const host of e.hosts) {
    const verdict = siteVerdict(host, settings);
    if (verdict !== "allowed" || !hasHostPermission(host)) return no("redirect_not_allowed");
  }

  if (settings.mimeTypes.length > 0) {
    if (!item.mime) return no("mime_unknown");
    if (!settings.mimeTypes.some((f) => mimeMatches(item.mime, f))) return no("mime_filtered");
  }
  if (settings.extensions.length > 0 && !settings.extensions.includes(ext))
    return no("type_filtered");
  if (settings.minSizeBytes > 0) {
    if (!(item.totalBytes > 0)) return no("size_unknown");
    if (item.totalBytes < settings.minSizeBytes) return no("too_small");
  }
  return { eligible: true };
}
