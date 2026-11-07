// Signed-in hand-off (BRW-05, ADR 0027): "Download with Downova using my sign-in".
//
// Pure decisions with no browser calls, so they run under vitest. The background worker asks for
// the optional `cookies` permission and the link's https origin inside the click, then calls these
// functions. Cookies are read from the browser's own cookie API for that one link, at the moment of
// sending (also on a retry). They are never written to extension storage: a hand-off record keeps
// only the cookie-store id, the origin, the browser kind and the remember choice, none of which is
// secret.
import type { BrowserKind, CookieInput, SessionInput } from "@dm/contracts";

/** Why a signed-in hand-off stays in the browser (stable codes; the popup translates them). */
export type SignInRefusal =
  | "rejected_signin_insecure"
  | "rejected_signin_cross_origin"
  | "rejected_signin_private"
  | "rejected_signin_permission"
  | "rejected_signin_store"
  | "rejected_signin_no_cookies";

/** The non-secret part of a signed-in hand-off, kept with its record. */
export interface SignInContext {
  origin: string;
  storeId: string;
  browser: BrowserKind;
  remember: boolean;
}

/** The subset of `chrome.cookies.Cookie` used here. */
export interface BrowserCookie {
  name: string;
  value: string;
  domain: string;
  hostOnly: boolean;
  path: string;
  secure: boolean;
  session: boolean;
  expirationDate?: number;
  storeId: string;
  /** Set for partitioned (CHIPS) cookies: those belong to an embedding site and are never sent. */
  partitionKey?: unknown;
}

export const MAX_COOKIES = 64;
export const MAX_SESSION_BYTES = 16 * 1024;

/** `scheme://host[:port]` of an https URL, or null. The default port is omitted (as `URL` does). */
export function httpsOrigin(raw: string): string | null {
  try {
    const u = new URL(raw);
    return u.protocol === "https:" && u.hostname !== "" ? u.origin : null;
  } catch {
    return null;
  }
}

/**
 * The scope rule checked at the click: an https link in a normal window, of the same origin
 * (scheme, host and port) as the frame it was clicked in. Anything else stays in the browser.
 */
export function checkScope(
  link: string,
  frameOrPage: string | undefined,
  incognito: boolean,
): { ok: true; origin: string } | { ok: false; code: SignInRefusal } {
  if (incognito) return { ok: false, code: "rejected_signin_private" };
  const origin = httpsOrigin(link);
  if (!origin) return { ok: false, code: "rejected_signin_insecure" };
  if (!frameOrPage || httpsOrigin(frameOrPage) !== origin)
    return { ok: false, code: "rejected_signin_cross_origin" };
  return { ok: true, origin };
}

/** The one cookie store holding `tabId`; none or more than one is ambiguous and refused. */
export function storeForTab(
  stores: readonly { id: string; tabIds: readonly number[] }[],
  tabId: number | undefined,
): string | null {
  if (tabId === undefined || tabId < 0) return null;
  const found = stores.filter((s) => s.tabIds.includes(tabId));
  return found.length === 1 ? (found[0]?.id ?? null) : null;
}

/**
 * The cookies offered for `url`: from the browser's answer for that URL and store, without
 * partitioned or expired cookies, and within the contract's bounds. Returns null when nothing
 * usable remains (the link is then not treated as signed in).
 */
export function selectCookies(
  cookies: readonly BrowserCookie[],
  storeId: string,
  nowMs: number,
): CookieInput[] | null {
  const out: CookieInput[] = [];
  let bytes = 0;
  for (const c of cookies) {
    if (c.storeId !== storeId || c.partitionKey !== undefined) continue;
    const expiresMs =
      !c.session && typeof c.expirationDate === "number"
        ? Math.floor(c.expirationDate * 1000)
        : undefined;
    if (expiresMs !== undefined && expiresMs <= nowMs) continue;
    if (
      c.name === "" ||
      c.name.length > 256 ||
      /[;=\r\n]/.test(c.name) ||
      c.value.length > 4096 ||
      c.domain === "" ||
      !c.path.startsWith("/")
    )
      continue;
    const size = c.name.length + c.value.length + c.domain.length + c.path.length;
    if (out.length >= MAX_COOKIES || bytes + size > MAX_SESSION_BYTES) break;
    bytes += size;
    out.push({
      name: c.name,
      value: c.value,
      domain: c.domain,
      host_only: c.hostOnly,
      path: c.path,
      secure: c.secure,
      ...(expiresMs !== undefined ? { expires_at_ms: String(expiresMs) } : {}),
    });
  }
  return out.length > 0 ? out : null;
}

/** Chrome or Edge, from the user agent (Edge says "Edg/"). */
export function browserKind(userAgent: string): BrowserKind {
  return /\bEdg\//.test(userAgent) ? "edge" : "chrome";
}

/** A profile id is 16+ lowercase hex characters, random per browser profile, never derived. */
export function isProfileId(s: unknown): s is string {
  return typeof s === "string" && /^[0-9a-f]{16,64}$/.test(s);
}

export function session(
  context: SignInContext,
  profileId: string,
  cookies: CookieInput[],
): SessionInput {
  const [first, ...rest] = cookies;
  if (!first) throw new Error("a session needs a cookie");
  return {
    origin: context.origin,
    browser: context.browser,
    profile_id: profileId,
    cookies: [first, ...rest],
    remember: context.remember,
  };
}
