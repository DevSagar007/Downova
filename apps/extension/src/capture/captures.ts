// Automatic capture state machine (B2.13, ADR 0019).
//
// Durable business state lives in `storage.local` (never only in worker memory). One record per
// browser download (id + start time), written before any native message, with its idempotency
// key: duplicate events, a stopped worker, a browser restart and a lost acknowledgement all lead
// back to the same record and the same key, so the service creates at most one job per browser
// download. The browser download is cancelled only after a matching acceptance for this record
// and a recheck that taking over still applies; otherwise it simply continues. Browser history
// is never erased and browser files are never removed.
import type { NativeHostRequest } from "@dm/contracts";
import { displayUrl, interpret, type SendResult } from "../handoff.js";
import { correlate, type Evidence } from "./evidence.js";
import { decide, siteVerdict, type CaptureSettings, type Item, type Reason } from "./policy.js";

export type CaptureState =
  "evaluating" | "browser_owned" | "sending" | "accepted" | "uncertain" | "failed";

/** What happened to the browser's own copy (a separate fact from Downova's acceptance). */
export type BrowserSide =
  "running" | "cancelled_by_downova" | "finished_in_browser" | "cancelled_by_user" | "kept";

export interface CaptureRecord {
  /** `cap_<download id>_<start time>`: a browser download, across events and restarts. */
  id: string;
  downloadId: number;
  /** Sent only to the native host; shown through `display` (query and fragment hidden). */
  url: string;
  display: string;
  state: CaptureState;
  reason: Reason | null;
  /** Fixed before the first send; every retry of this capture reuses it. */
  idempotencyKey: string;
  jobId: string | null;
  browser: BrowserSide;
  /**
   * Set (and stored) just before Downova cancels the browser's copy: an interruption reported
   * after it is Downova's cancel, not the user's — even across a worker restart.
   */
  cancelRequested?: boolean;
  attempts: number;
  createdAt: number;
  updatedAt: number;
}

export const CAPTURES_KEY = "captures";
export const MAX_CAPTURES = 50;
export const CAPTURE_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
/** At most this many captures evaluate or send at once; more stay in the browser ("busy"). */
export const MAX_IN_FLIGHT = 3;
/** Automatic sends per capture (the second only after an unconfirmed first, same key). */
export const MAX_ATTEMPTS = 2;
/** How long a decision waits for the browser's filename/danger verdict and request evidence. */
export const DECIDE_DEADLINE_MS = 5_000;
export const SEND_TIMEOUT_MS = 30_000;

export interface BrowserItem extends Item {
  startTime: string;
  /** `USER_CANCELED` etc. when interrupted. */
  error?: string | undefined;
}

export interface CaptureDeps {
  load(): Promise<CaptureRecord[]>;
  save(records: CaptureRecord[]): Promise<void>;
  settings(): Promise<CaptureSettings>;
  /** Whether the optional APIs are granted and, per host, the host permission. */
  hasApis(): Promise<boolean>;
  hostPermissions(): Promise<(host: string) => boolean>;
  evidence(): Promise<Evidence[]>;
  /** The browser's current view of a download, or null when it no longer exists. */
  item(id: number): Promise<BrowserItem | null>;
  cancel(id: number): Promise<void>;
  send(message: NativeHostRequest, timeoutMs: number): Promise<SendResult>;
  now(): number;
  token(): string;
  sleep(ms: number): Promise<void>;
}

const ACTIVE: readonly CaptureState[] = ["evaluating", "sending"];

export function recordId(item: { id: number; startTime: string }): string {
  return `cap_${item.id}_${item.startTime.replace(/[^0-9A-Za-z]/g, "")}`;
}

function settle(records: CaptureRecord[], now: number): CaptureRecord[] {
  return records.filter(
    (r) => ACTIVE.includes(r.state) || now - r.updatedAt <= CAPTURE_RETENTION_MS,
  );
}

export class Captures {
  // Serializes read-modify-write of the stored list within this worker's lifetime.
  private queue: Promise<unknown> = Promise.resolve();
  constructor(private readonly deps: CaptureDeps) {}

  private update<T>(f: (records: CaptureRecord[]) => [CaptureRecord[], T]): Promise<T> {
    const next = this.queue.then(async () => {
      const [records, out] = f(settle(await this.deps.load(), this.deps.now()));
      await this.deps.save(records.slice(0, MAX_CAPTURES));
      return out;
    });
    this.queue = next.catch(() => undefined);
    return next;
  }

  private patch(id: string, change: Partial<CaptureRecord>): Promise<CaptureRecord | null> {
    return this.update((records) => {
      let out: CaptureRecord | null = null;
      const next = records.map((r) => {
        if (r.id !== id) return r;
        out = { ...r, ...change, updatedAt: this.deps.now() };
        return out;
      });
      return [next, out];
    });
  }

  list(): Promise<CaptureRecord[]> {
    return this.update((records) => [records, records]);
  }

  /** Removes every settled record (captures still in flight are kept). */
  clear(): Promise<void> {
    return this.update((records) => [records.filter((r) => ACTIVE.includes(r.state)), undefined]);
  }

  /**
   * `downloads.onCreated`. Duplicate events for the same download find the existing record and
   * do nothing. Nothing is recorded while capture is off, for incognito downloads or for
   * downloads another extension started.
   */
  async onCreated(created: BrowserItem): Promise<CaptureRecord | null> {
    const settings = await this.deps.settings();
    if (!settings.enabled || created.incognito || created.byExtensionId) return null;
    if (created.state !== "in_progress") return null;
    if (!(await this.deps.hasApis())) return null;
    const id = recordId(created);
    const now = this.deps.now();
    const inserted = await this.update((records) => {
      if (records.some((r) => r.id === id)) return [records, null];
      // Downloads from sites outside the allowlist are left alone without a record (only sites
      // the user chose appear in the activity list).
      const hosts = [created.url, created.finalUrl].map(hostOf);
      if (hosts.some((h) => h !== null && siteVerdict(h, settings) === "excluded")) {
        return [records, null];
      }
      if (!hosts.some((h) => h !== null && siteVerdict(h, settings) === "allowed")) {
        return [records, null];
      }
      const busy = records.filter((r) => ACTIVE.includes(r.state)).length >= MAX_IN_FLIGHT;
      const record: CaptureRecord = {
        id,
        downloadId: created.id,
        url: created.url,
        display: displayUrl(created.url),
        state: busy ? "browser_owned" : "evaluating",
        reason: busy ? "busy" : null,
        idempotencyKey: `cap_${this.deps.token()}`,
        jobId: null,
        browser: "running",
        attempts: 0,
        createdAt: now,
        updatedAt: now,
      };
      return [[record, ...records], record];
    });
    if (!inserted || inserted.state !== "evaluating") return inserted;
    return this.evaluate(inserted.id);
  }

  /** `downloads.onChanged`: notes how the browser's copy ended (used by the rechecks). */
  async onChanged(delta: {
    id: number;
    state?: string | undefined;
    error?: string | undefined;
  }): Promise<void> {
    if (delta.state !== "complete" && delta.state !== "interrupted") return;
    const side = (r: CaptureRecord): BrowserSide =>
      delta.state === "complete"
        ? "finished_in_browser"
        : delta.error === "USER_CANCELED"
          ? r.cancelRequested
            ? "cancelled_by_downova"
            : "cancelled_by_user"
          : "kept";
    await this.update((records) => [
      records.map((r) =>
        r.downloadId === delta.id && r.browser === "running"
          ? { ...r, browser: side(r), updatedAt: this.deps.now() }
          : r,
      ),
      undefined,
    ]);
  }

  /** Permission revoked or capture disabled: in-flight evaluations stay in the browser. */
  async abandonInFlight(reason: "permission_revoked" | "capture_disabled"): Promise<void> {
    await this.update((records) => [
      records.map((r) =>
        r.state === "evaluating"
          ? { ...r, state: "browser_owned" as const, reason, updatedAt: this.deps.now() }
          : r,
      ),
      undefined,
    ]);
  }

  /**
   * Waits (bounded) for the browser's filename and danger verdict and for the request evidence,
   * then decides. Ineligible downloads become `browser_owned` with the reason.
   */
  private async evaluate(id: string): Promise<CaptureRecord | null> {
    const started = this.deps.now();
    const record = (await this.list()).find((r) => r.id === id);
    if (!record || record.state !== "evaluating") return record ?? null;
    for (;;) {
      const settings = await this.deps.settings();
      if (!settings.enabled) return this.ownedBy(id, "capture_disabled");
      if (!(await this.deps.hasApis())) return this.ownedBy(id, "permission_revoked");
      const item = await this.deps.item(record.downloadId);
      if (!item || item.state !== "in_progress") {
        return this.patch(id, {
          state: "browser_owned",
          reason: null,
          browser: item?.state === "complete" ? "finished_in_browser" : "kept",
        });
      }
      const late = this.deps.now() - started >= DECIDE_DEADLINE_MS;
      // The danger verdict is only final once the browser chose the file name.
      if (item.filename === "" && !late) {
        await this.deps.sleep(250);
        continue;
      }
      const found = correlate(await this.deps.evidence(), item, record.createdAt);
      if (found.kind === "pending" && !late) {
        await this.deps.sleep(250);
        continue;
      }
      const hostPermission = await this.deps.hostPermissions();
      const decision = decide(
        item.filename === "" ? { ...item, danger: "unknown" } : item,
        found.kind === "pending" ? { kind: "none" } : found,
        settings,
        hostPermission,
      );
      if (!decision.eligible) return this.ownedBy(id, decision.reason);
      return this.deliver(id);
    }
  }

  private ownedBy(id: string, reason: Reason): Promise<CaptureRecord | null> {
    return this.patch(id, { state: "browser_owned", reason });
  }

  /** Sends (or re-sends, same key) the add; on acceptance, takes the download over. */
  private async deliver(id: string): Promise<CaptureRecord | null> {
    // The attempt is counted before sending, so a worker stopped mid-send cannot resend forever.
    const current = await this.update((records) => {
      let out: CaptureRecord | null = null;
      const next = records.map((r) => {
        if (r.id !== id) return r;
        out = {
          ...r,
          state: "sending" as const,
          attempts: r.attempts + 1,
          updatedAt: this.deps.now(),
        };
        return out;
      });
      return [next, out as CaptureRecord | null];
    });
    if (!current) return null;
    const requestId = `req_${this.deps.token()}`;
    const result = await this.deps.send(
      {
        v: 1,
        request_id: requestId,
        op: "add",
        url: current.url,
        idempotency_key: current.idempotencyKey,
      },
      SEND_TIMEOUT_MS,
    );
    const outcome = interpret(result, requestId);
    const attempts = current.attempts;
    switch (outcome.state) {
      case "accepted": {
        await this.patch(id, { state: "accepted", jobId: outcome.jobId, attempts, reason: null });
        return this.takeOver(id);
      }
      case "rejected":
        return this.patch(id, { state: "failed", reason: "downova_rejected", attempts });
      case "unavailable":
        return this.patch(id, { state: "failed", reason: "downova_unavailable", attempts });
      default: {
        // Unconfirmed: the job may exist. One more automatic try with the same key (it can only
        // return that job), and only while the browser download is still running.
        const item = await this.deps.item(current.downloadId);
        const settings = await this.deps.settings();
        if (attempts < MAX_ATTEMPTS && item?.state === "in_progress" && settings.enabled) {
          return this.deliver(id);
        }
        return this.patch(id, { state: "uncertain", attempts });
      }
    }
  }

  /**
   * After a matching acceptance: cancel the browser download only if taking over still
   * applies — capture on, permissions held, the same download still running and still safe.
   * Otherwise the browser's copy is left alone and the record says why.
   */
  private async takeOver(id: string): Promise<CaptureRecord | null> {
    const record = (await this.list()).find((r) => r.id === id);
    if (!record || record.state !== "accepted" || record.browser !== "running") {
      return record ?? null;
    }
    const settings = await this.deps.settings();
    if (!settings.enabled) return this.patch(id, { browser: "kept", reason: "capture_disabled" });
    if (!(await this.deps.hasApis())) {
      return this.patch(id, { browser: "kept", reason: "permission_revoked" });
    }
    const item = await this.deps.item(record.downloadId);
    if (!item) return this.patch(id, { browser: "kept" });
    if (item.state === "complete") return this.patch(id, { browser: "finished_in_browser" });
    if (item.state === "interrupted") {
      return this.patch(id, {
        browser:
          item.error === "USER_CANCELED"
            ? record.cancelRequested
              ? "cancelled_by_downova"
              : "cancelled_by_user"
            : "kept",
      });
    }
    if (item.url !== record.url || item.danger !== "safe") {
      return this.patch(id, { browser: "kept", reason: "browser_warning" });
    }
    // Stored before cancelling: a worker stopped right after the cancel still attributes it.
    await this.patch(id, { cancelRequested: true });
    try {
      await this.deps.cancel(record.downloadId);
    } catch {
      return this.patch(id, { browser: "kept" });
    }
    // `cancel` resolves even when the download completed meanwhile: report what really happened.
    const after = await this.deps.item(record.downloadId);
    return this.patch(id, {
      browser: after?.state === "complete" ? "finished_in_browser" : "cancelled_by_downova",
    });
  }

  /**
   * Worker (re)start: continues captures interrupted at any boundary. Evaluations and sends
   * resume with the stored key; an acceptance whose takeover did not happen is rechecked.
   */
  async resume(): Promise<void> {
    // At worker start nothing of this worker is in flight: every active record was interrupted.
    const records = await this.list();
    const now = this.deps.now();
    for (const r of records) {
      if (r.state === "accepted" && r.browser === "running") {
        await this.takeOver(r.id);
      } else if (r.state === "sending" || r.state === "evaluating") {
        const item = await this.deps.item(r.downloadId);
        if (!item || item.state !== "in_progress") {
          // Nothing to take over any more; an unanswered send stays honestly uncertain.
          await this.patch(r.id, {
            state: r.state === "sending" ? "uncertain" : "browser_owned",
            browser: item?.state === "complete" ? "finished_in_browser" : "kept",
          });
        } else if (r.state === "sending") {
          // Re-sent with the same key: it can only return the job the lost answer was about.
          if (r.attempts >= MAX_ATTEMPTS) await this.patch(r.id, { state: "uncertain" });
          else await this.deliver(r.id);
        } else {
          await this.patch(r.id, { updatedAt: now });
          await this.evaluate(r.id);
        }
      }
    }
  }
}

function hostOf(raw: string): string | null {
  try {
    return new URL(raw).hostname;
  } catch {
    return null;
  }
}
