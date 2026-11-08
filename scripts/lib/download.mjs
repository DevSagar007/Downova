// Bounded, verified downloads of pinned build tools (CI reliability; GitHub release assets
// sometimes answer 504 for a while). Shared by scripts/fetch-aria2.mjs and scripts/package.mjs.
//
// Retries only what can succeed on a later attempt: network errors, per-attempt timeouts and
// HTTP 408, 429 and 5xx. Every other HTTP status, and any size or SHA-256 mismatch, fails at once
// (a checksum mismatch is never "retried away"). Attempts, per-attempt time and the total budget
// are finite; Retry-After is honoured only within the remaining budget. Logs name the tool, the
// attempt and a sanitized URL (no query string, no credentials).
import { createHash } from "node:crypto";

export class DownloadError extends Error {
  constructor(message, { transient = false } = {}) {
    super(message);
    this.transient = transient;
  }
}

/** The URL without credentials or query (signed redirects carry tokens in the query). */
export function sanitizeUrl(url) {
  try {
    const u = new URL(url);
    return `${u.protocol}//${u.host}${u.pathname}`;
  } catch {
    return "<invalid url>";
  }
}

export const isTransientStatus = (status) =>
  status === 408 || status === 429 || (status >= 500 && status <= 599);

/** Seconds or HTTP-date from a Retry-After header, in ms (null when absent or invalid). */
export function retryAfterMs(header, now = Date.now()) {
  if (!header) return null;
  const secs = Number(header);
  if (Number.isFinite(secs) && secs >= 0) return secs * 1000;
  const at = Date.parse(header);
  return Number.isFinite(at) ? Math.max(0, at - now) : null;
}

/** Exponential backoff (5 s, 10 s, 20 s, 40 s, then 60 s), or Retry-After when given. */
export function backoffMs(attempt, retryAfter) {
  if (retryAfter !== null && retryAfter !== undefined) return retryAfter;
  return Math.min(60_000, 5000 * 2 ** (attempt - 1));
}

/**
 * Downloads `url` and returns its bytes once their size and SHA-256 match the pin.
 * `options`: tool (name for logs), size, sha256, attempts (5), timeoutMs per attempt (120 s),
 * budgetMs total (10 min), and injectable fetchImpl / sleep / now / log for tests.
 */
export async function downloadVerified(url, options) {
  const {
    tool,
    size,
    sha256,
    attempts = 5,
    timeoutMs = 120_000,
    budgetMs = 600_000,
    fetchImpl = fetch,
    sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
    now = Date.now,
    log = (line) => console.error(line),
  } = options;
  const shown = sanitizeUrl(url);
  const started = now();
  let last = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    let wait = null;
    try {
      const response = await fetchImpl(url, { signal: AbortSignal.timeout(timeoutMs) });
      if (!response.ok) {
        const transient = isTransientStatus(response.status);
        last = new DownloadError(`${tool}: HTTP ${response.status} from ${shown}`, { transient });
        if (!transient) throw last;
        wait = retryAfterMs(response.headers?.get?.("retry-after") ?? null, now());
        await response.body?.cancel?.().catch(() => {});
      } else {
        const bytes = Buffer.from(await response.arrayBuffer());
        const digest = createHash("sha256").update(bytes).digest("hex");
        if ((size !== undefined && bytes.length !== size) || (sha256 && digest !== sha256)) {
          // Never retried: a different file is not a network hiccup.
          throw new DownloadError(
            `${tool}: checksum mismatch from ${shown} (size ${bytes.length}, sha256 ${digest}); refusing it`,
          );
        }
        if (attempt > 1) log(`${tool}: downloaded on attempt ${attempt}/${attempts}`);
        return bytes;
      }
    } catch (e) {
      if (e instanceof DownloadError && !e.transient) throw e;
      if (!(e instanceof DownloadError)) {
        // fetch rejects with TypeError on network failures and with a timeout DOMException.
        const reason =
          e?.name === "TimeoutError" || e?.name === "AbortError"
            ? `timed out after ${timeoutMs} ms`
            : `network error (${e?.cause?.code ?? e?.name ?? "unknown"})`;
        last = new DownloadError(`${tool}: ${reason} from ${shown}`, { transient: true });
      }
    }
    if (attempt === attempts) break;
    const delay = backoffMs(attempt, wait);
    const remaining = budgetMs - (now() - started);
    if (delay > remaining) {
      log(
        `${last.message}; the ${Math.round(budgetMs / 1000)} s budget does not allow another attempt`,
      );
      break;
    }
    log(
      `${last.message}; attempt ${attempt}/${attempts} failed, retrying in ${Math.round(delay / 1000)} s`,
    );
    await sleep(delay);
  }
  throw new DownloadError(
    `${last?.message ?? `${tool}: download failed`} (gave up after ${attempts} attempt(s) or the time budget)`,
    {
      transient: true,
    },
  );
}

/**
 * Whether a failed `tauri build` failed only while downloading one of its pinned bundling tools
 * because of a transient network or server error (positively identified from its output), so
 * the command may be run again. Compile errors, hash mismatches and anything else: never.
 * The tool names are those the pinned Tauri CLI 2.12.3 downloads; the error forms are those of its
 * HTTP client (ureq 3.x: "http status: 504", "timeout: …", "io: …", "host not found").
 */
export function isTransientToolDownloadFailure(output) {
  const tail = output.slice(-16_384);
  if (
    /hash mismatch|mis-hashed|error\[E\d{4}\]|could not compile|failed to bundle project.*(?:sign|notar)/i.test(
      tail,
    )
  )
    return false;
  const tool =
    /(nsis-3\.\d+\.zip|wix3\d*-binaries\.zip|nsis_tauri_utils|linuxdeploy[^\s"']*\.AppImage|AppRun[^\s"']*|appimage[^\s"']*plugin[^\s"']*)/i.test(
      tail,
    );
  const transient =
    /http status:\s*(408|429|5\d\d)\b|status code:?\s*(408|429|5\d\d)\b|\b(408|429|50[0-4])\s+(Request Timeout|Too Many Requests|Internal Server Error|Bad Gateway|Service Unavailable|Gateway Time-?out)|timeout:|timed out|connection (reset|refused|closed)|host not found|failed to lookup address|unexpected eof/i.test(
      tail,
    );
  return tool && transient;
}
