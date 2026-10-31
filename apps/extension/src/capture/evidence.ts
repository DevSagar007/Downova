// Request evidence for automatic capture (B2.13, ADR 0019).
//
// Non-blocking webRequest observations (never webRequestBlocking) of the requests that could
// become downloads: the method, whether the browser sent cookies or credentials (header NAMES
// only — values are never read or stored), the redirect hosts and the final status. Kept in
// `storage.session` (memory-only, survives a worker restart, cleared when the browser closes),
// bounded in count and age. A download is matched to exactly one completed observation of its
// URL; zero or several matches leave it in the browser.

export interface Evidence {
  requestId: string;
  /** Current URL of the request (after any redirect so far). */
  url: string;
  method: string;
  /** Every host the request visited, start to finish. */
  hosts: string[];
  sentCookies: boolean;
  sentCredentials: boolean;
  /** Final status; 0 until the response headers arrived. */
  status: number;
  /** When the response headers arrived (0 before). */
  completedAt: number;
  startedAt: number;
}

export const MAX_EVIDENCE = 200;
/** Observations older than this are dropped (a download is created right after its headers). */
export const EVIDENCE_TTL_MS = 2 * 60 * 1000;

const CREDENTIAL_HEADERS = ["authorization", "proxy-authorization"];

function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

export interface RequestStart {
  requestId: string;
  url: string;
  method: string;
  requestHeaders?: { name: string }[] | undefined;
}

/** Folds one onSendHeaders observation in (a new request, or a redirected one re-sending). */
export function onSendHeaders(list: Evidence[], d: RequestStart, now: number): Evidence[] {
  const names = (d.requestHeaders ?? []).map((h) => h.name.toLowerCase());
  const cookies = names.includes("cookie");
  const credentials = names.some((n) => CREDENTIAL_HEADERS.includes(n));
  const host = hostOf(d.url);
  const existing = list.find((e) => e.requestId === d.requestId);
  if (existing) {
    // The same request after a redirect: anything sent on any hop counts.
    return list.map((e) =>
      e.requestId === d.requestId
        ? {
            ...e,
            url: d.url,
            method: d.method,
            sentCookies: e.sentCookies || cookies,
            sentCredentials: e.sentCredentials || credentials,
            hosts: host && e.hosts.at(-1) !== host ? [...e.hosts, host] : e.hosts,
          }
        : e,
    );
  }
  const next: Evidence = {
    requestId: d.requestId,
    url: d.url,
    method: d.method,
    hosts: host ? [host] : [],
    sentCookies: cookies,
    sentCredentials: credentials,
    status: 0,
    completedAt: 0,
    startedAt: now,
  };
  return prune([next, ...list], now);
}

/** A redirect: the next hop's host joins the chain (its headers arrive with onSendHeaders). */
export function onBeforeRedirect(
  list: Evidence[],
  d: { requestId: string; redirectUrl: string },
): Evidence[] {
  const host = hostOf(d.redirectUrl);
  return list.map((e) =>
    e.requestId === d.requestId
      ? {
          ...e,
          url: d.redirectUrl,
          hosts: host && e.hosts.at(-1) !== host ? [...e.hosts, host] : e.hosts,
        }
      : e,
  );
}

export function onHeadersReceived(
  list: Evidence[],
  d: { requestId: string; url: string; statusCode: number },
  now: number,
): Evidence[] {
  return list.map((e) =>
    e.requestId === d.requestId ? { ...e, url: d.url, status: d.statusCode, completedAt: now } : e,
  );
}

export function prune(list: Evidence[], now: number): Evidence[] {
  return list.filter((e) => now - e.startedAt <= EVIDENCE_TTL_MS).slice(0, MAX_EVIDENCE);
}

/**
 * The single completed observation of `finalUrl` (or `url`) that started before `createdAt`.
 * Several distinct observations of the same URL are ambiguous: the request that became this
 * download cannot be told apart, so it stays in the browser.
 */
export function correlate(
  list: Evidence[],
  item: { url: string; finalUrl: string },
  createdAt: number,
):
  | { kind: "match"; evidence: Evidence }
  | { kind: "none" }
  | { kind: "ambiguous" }
  | { kind: "pending" } {
  const target = item.finalUrl || item.url;
  const candidates = list.filter(
    (e) =>
      e.url === target &&
      e.startedAt <= createdAt + 1_000 &&
      createdAt - e.startedAt <= EVIDENCE_TTL_MS,
  );
  if (candidates.length > 1) return { kind: "ambiguous" };
  const [only] = candidates;
  if (!only) return { kind: "none" };
  return only.completedAt > 0 ? { kind: "match", evidence: only } : { kind: "pending" };
}
