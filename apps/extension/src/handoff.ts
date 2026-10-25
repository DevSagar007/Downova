// Explicit link hand-off to the Downova native host (B0.6, BRW-01).
//
// Pure logic with injected dependencies so it runs under vitest without a browser. The record
// list in extension storage is the source of truth: a hand-off is written there (state
// "sending", with its idempotency key) before the native message goes out, so a service worker
// that is stopped mid-request leaves a record that later reads as "uncertain" and can be retried
// with the same key. Nothing here touches browser downloads.
import type { NativeHostAdd, NativeHostOutcome, NativeHostResponse } from "@dm/contracts";

export const HOST_NAME = "com.downova.host";
/** Bound on one native round trip; the host itself gives up on the service after ~16 s. */
export const SEND_TIMEOUT_MS = 30_000;
/** A "sending" record older than this was interrupted (worker stopped) and is uncertain. */
export const STALE_SENDING_MS = SEND_TIMEOUT_MS + 5_000;
export const MAX_RECORDS = 20;
export const MAX_URL = 8192;

export type HandoffState = "sending" | "accepted" | "rejected" | "unavailable" | "uncertain";

export interface HandoffRecord {
  id: string;
  /** Shown to the user; never sent anywhere except the native host. */
  url: string;
  /** Kept for every retry of this action. */
  idempotencyKey: string;
  state: HandoffState;
  detail: string;
  jobId: string | null;
  attempts: number;
  createdAt: number;
  updatedAt: number;
}

export type SendResult =
  { kind: "response"; value: unknown } | { kind: "timeout" } | { kind: "error"; message: string };

export interface Deps {
  load(): Promise<HandoffRecord[]>;
  save(records: HandoffRecord[]): Promise<void>;
  send(message: NativeHostAdd, timeoutMs: number): Promise<SendResult>;
  now(): number;
  /** Random URL-safe token (at least 16 characters). */
  token(): string;
}

/** Syntactic check matching the host: absolute http(s), a host, no credentials, no spaces. */
export function checkLink(raw: string): { ok: true; url: string } | { ok: false; reason: string } {
  if (raw.length > MAX_URL) return { ok: false, reason: "The link is too long." };
  let u: URL;
  try {
    u = new URL(raw);
  } catch {
    return { ok: false, reason: "This is not a valid link." };
  }
  if (u.protocol !== "http:" && u.protocol !== "https:")
    return {
      ok: false,
      reason: `Only http and https links can be handed to Downova (this is ${u.protocol}).`,
    };
  if (u.username !== "" || u.password !== "")
    return {
      ok: false,
      reason: "Links with an embedded user name or password stay in the browser.",
    };
  if (u.hostname === "") return { ok: false, reason: "The link has no host." };
  // The link is sent exactly as given; the parser above only validated it.
  if (/[\s\p{Cc}]/u.test(raw))
    return { ok: false, reason: "The link contains spaces or control characters." };
  return { ok: true, url: raw };
}

const MESSAGES: Record<Exclude<HandoffState, "sending">, string> = {
  accepted:
    "Downova accepted the link. The transfer has not necessarily started; follow it in the Downova app.",
  rejected: "Downova did not accept this link.",
  unavailable: "Downova is not available. Nothing was sent.",
  uncertain:
    "No confirmation arrived. The download may or may not have been added; retry to find out (no duplicate is created).",
};

export function describe(record: HandoffRecord): string {
  if (record.state === "sending") return "Sending to Downova…";
  return record.detail || MESSAGES[record.state];
}

export function canRetry(record: HandoffRecord): boolean {
  return record.state === "uncertain" || record.state === "unavailable";
}

const OUTCOMES: readonly NativeHostOutcome[] = [
  "service_ready",
  "accepted",
  "rejected",
  "unavailable",
  "uncertain",
];

/** Maps one native result to a final state. Anything unexpected after sending is uncertain. */
export function interpret(
  result: SendResult,
  requestId: string,
): { state: Exclude<HandoffState, "sending">; detail: string; jobId: string | null } {
  if (result.kind === "timeout")
    return { state: "uncertain", detail: MESSAGES.uncertain, jobId: null };
  if (result.kind === "error") {
    // Chrome reports a missing or forbidden host before anything reaches it.
    if (/not found|forbidden/i.test(result.message))
      return {
        state: "unavailable",
        detail:
          "Downova's browser helper is not installed for this browser. Install or repair Downova, then retry.",
        jobId: null,
      };
    return { state: "uncertain", detail: MESSAGES.uncertain, jobId: null };
  }
  const r = result.value as Partial<NativeHostResponse> | null;
  if (
    typeof r !== "object" ||
    r === null ||
    r.v !== 1 ||
    r.request_id !== requestId ||
    !OUTCOMES.includes(r.outcome as NativeHostOutcome)
  )
    return { state: "uncertain", detail: MESSAGES.uncertain, jobId: null };
  const hostDetail = r.error?.detail ?? "";
  switch (r.outcome) {
    case "accepted":
      if (typeof r.job?.job_id !== "string")
        return { state: "uncertain", detail: MESSAGES.uncertain, jobId: null };
      return { state: "accepted", detail: MESSAGES.accepted, jobId: r.job.job_id };
    case "rejected":
      return {
        state: "rejected",
        detail: hostDetail ? `${MESSAGES.rejected} ${capitalise(hostDetail)}.` : MESSAGES.rejected,
        jobId: null,
      };
    case "unavailable":
      return {
        state: "unavailable",
        detail: hostDetail ? `${capitalise(hostDetail)}.` : MESSAGES.unavailable,
        jobId: null,
      };
    default:
      return { state: "uncertain", detail: MESSAGES.uncertain, jobId: null };
  }
}

function capitalise(s: string): string {
  const t = s.replace(/\.$/, "");
  return t.charAt(0).toUpperCase() + t.slice(1);
}

/** Marks interrupted "sending" records uncertain (call on worker start and before reads). */
export function settleStale(records: HandoffRecord[], now: number): HandoffRecord[] {
  return records.map((r) =>
    r.state === "sending" && now - r.updatedAt > STALE_SENDING_MS
      ? { ...r, state: "uncertain", detail: MESSAGES.uncertain, updatedAt: now }
      : r,
  );
}

export class Handoffs {
  // Serialises read-modify-write of the stored list within this worker's lifetime.
  private queue: Promise<unknown> = Promise.resolve();
  constructor(private readonly deps: Deps) {}

  private update<T>(f: (records: HandoffRecord[]) => [HandoffRecord[], T]): Promise<T> {
    const next = this.queue.then(async () => {
      const [records, out] = f(settleStale(await this.deps.load(), this.deps.now()));
      await this.deps.save(records.slice(0, MAX_RECORDS));
      return out;
    });
    this.queue = next.catch(() => undefined);
    return next;
  }

  list(): Promise<HandoffRecord[]> {
    return this.update((records) => [records, records]);
  }

  /** A new explicit action: new idempotency key. Invalid links are recorded as rejected. */
  async start(rawUrl: string): Promise<HandoffRecord> {
    const now = this.deps.now();
    const checked = checkLink(rawUrl);
    const record: HandoffRecord = {
      id: `h_${this.deps.token()}`,
      url: rawUrl.slice(0, MAX_URL),
      idempotencyKey: `ext_${this.deps.token()}`,
      state: checked.ok ? "sending" : "rejected",
      detail: checked.ok ? "" : checked.reason,
      jobId: null,
      attempts: 0,
      createdAt: now,
      updatedAt: now,
    };
    await this.update((records) => [[record, ...records], null]);
    return checked.ok ? this.deliver(record.id) : record;
  }

  /** Retries an uncertain or unavailable hand-off with its original idempotency key. */
  async retry(id: string): Promise<HandoffRecord | null> {
    const ok = await this.update((records) => {
      const r = records.find((x) => x.id === id);
      if (!r || !canRetry(r)) return [records, false];
      const now = this.deps.now();
      return [
        records.map((x) =>
          x.id === id ? { ...x, state: "sending", detail: "", updatedAt: now } : x,
        ),
        true,
      ];
    });
    return ok ? this.deliver(id) : null;
  }

  dismiss(id: string): Promise<void> {
    return this.update((records) => [
      records.filter((r) => r.id !== id || r.state === "sending"),
      undefined,
    ]);
  }

  private async deliver(id: string): Promise<HandoffRecord> {
    const current = (await this.list()).find((r) => r.id === id);
    if (!current) throw new Error("hand-off record disappeared");
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
    return this.update((records) => {
      let updated = current;
      const next = records.map((r) => {
        if (r.id !== id) return r;
        updated = { ...r, ...outcome, attempts: r.attempts + 1, updatedAt: this.deps.now() };
        return updated;
      });
      return [next, updated];
    });
  }
}

/** URL-safe random token from the platform CSPRNG. */
export function randomToken(bytes = 16): string {
  const b = crypto.getRandomValues(new Uint8Array(bytes));
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}
