// Which row commands apply to a download, from its service snapshot only (design.md §6).
// Manual pause (`desired_state`) is distinct from policy blockers and from terminal phases: a
// failed download is never offered as resumable (adding its link again starts it over).
import type { DownloadSnapshot } from "@dm/contracts";
import type { MessageKey } from "../../i18n/messages";

/** null = the command applies; otherwise the reason it does not. */
export type Eligibility = MessageKey | null;

function ended(d: DownloadSnapshot): Eligibility {
  if (d.phase === "completed") return "unavailable.finished";
  if (d.phase === "cancelled" || d.desired_state === "cancelled") return "unavailable.cancelled";
  return null;
}

export function canPause(d: DownloadSnapshot | undefined): Eligibility {
  if (!d) return "unavailable.noSelection";
  if (d.phase === "failed") return "unavailable.failed";
  return ended(d) ?? (d.desired_state === "paused" ? "unavailable.alreadyPaused" : null);
}

export function canResume(d: DownloadSnapshot | undefined): Eligibility {
  if (!d) return "unavailable.noSelection";
  if (d.phase === "failed") return "unavailable.failedResume";
  return ended(d) ?? (d.desired_state === "paused" ? null : "unavailable.notPaused");
}

/** Mirrors `dm_domain::validate::check_url` so the user sees the reason before sending. */
export const MAX_URL = 8192;

export type UrlProblem = "empty" | "tooLong" | "spaces" | "scheme" | "credentials" | "host";

export function checkUrl(url: string): UrlProblem | null {
  if (url.length === 0) return "empty";
  if (new TextEncoder().encode(url).length > MAX_URL) return "tooLong";
  // Any whitespace or control character (Rust `char::is_whitespace` / `is_control`).
  if (/[\s\p{Cc}]/u.test(url)) return "spaces";
  const lower = url.toLowerCase();
  const rest = lower.startsWith("https://")
    ? lower.slice(8)
    : lower.startsWith("http://")
      ? lower.slice(7)
      : null;
  if (rest === null) return "scheme";
  const authority = rest.split(/[/?#]/, 1)[0] ?? "";
  if (authority.includes("@")) return "credentials";
  if (authority === "" || authority.startsWith(":")) return "host";
  return null;
}

/** A new idempotency key for one add request (`[A-Za-z0-9_-]`, 8–128 characters). */
export function newIdempotencyKey(): string {
  return `ui-${crypto.randomUUID()}`;
}

/** Cancel stops and discards an unfinished download. A failed or finished one has nothing to stop;
 * once the file is being placed (phase finalizing) the service refuses, so it is not offered. */
export function canCancel(d: DownloadSnapshot | undefined): Eligibility {
  if (!d) return "unavailable.noSelection";
  if (d.phase === "failed") return "unavailable.failed";
  if (d.phase === "finalizing") return "unavailable.finalizing";
  return ended(d);
}

/** Remove from list: finished downloads only (ADR 0013). */
export function canRemove(d: DownloadSnapshot | undefined): Eligibility {
  if (!d) return "unavailable.noSelection";
  return ["completed", "failed", "cancelled"].includes(d.phase) ? null : "unavailable.notFinished";
}
