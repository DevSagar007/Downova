// Reconnect an existing signed-in download (BRW-05, ADR 0028).
//
// "Reconnect a Downova download with my sign-in" on an https page: after the permission prompt
// (in the click), the extension asks Downova which of this origin's downloads this browser
// profile may reconnect. The user then chooses one explicitly on the reconnect page (nothing is
// preselected, nothing is matched by name) and either "Reconnect" or "Reconnect and resume".
// Cookies are read only at that moment, for that origin and cookie store, and sent once.
//
// Records (non-secret: origin, store id, browser, the offered downloads' metadata, the choice and
// its operation id) live in `chrome.storage.session`: memory only, kept across a stopped worker.
// One operation id per (request, download): a retry after a lost acknowledgement reuses it, so the
// service changes nothing twice; choosing another download is a new operation.
import type {
  BrowserKind,
  NativeHostOutcome,
  NativeHostRequest,
  NativeHostResponse,
  ReconnectCandidate,
  SessionInput,
} from "@dm/contracts";
import type { SendResult } from "./handoff.js";
import type { SignInRefusal } from "./signin.js";

export const RECONNECTS_KEY = "reconnects";
export const RECONNECT_TIMEOUT_MS = 30_000;
const MAX_RECORDS = 5;
/** A request older than this is dropped (its candidates are stale). */
export const RECONNECT_TTL_MS = 30 * 60 * 1000;

export type ReconnectState =
  | "listing"
  | "choosing"
  | "none"
  | "sending"
  | "reconnected"
  | "refused"
  | "unavailable"
  | "uncertain";

export interface ReconnectRecord {
  id: string;
  origin: string;
  storeId: string;
  browser: BrowserKind;
  state: ReconnectState;
  candidates: ReconnectCandidate[];
  /** The download the user chose, and how. */
  jobId?: string;
  resume?: boolean;
  /** One per (record, download): reused by retries, never for another download. */
  operationId?: string;
  /** Why it was refused (an extension refusal code or the service's error kind). */
  code?: string;
  /** The phase Downova reported after reconnecting (a failed download needs Retry there). */
  phase?: string;
  createdAt: number;
  updatedAt: number;
}

export interface ReconnectDeps {
  load(): Promise<ReconnectRecord[]>;
  save(records: ReconnectRecord[]): Promise<void>;
  send(message: NativeHostRequest, timeoutMs: number): Promise<SendResult>;
  /** The browser's current sign-in for the record's origin and store, read now, never stored. */
  session(
    record: ReconnectRecord,
  ): Promise<{ ok: true; session: SessionInput } | { ok: false; code: SignInRefusal }>;
  profileId(): Promise<string>;
  now(): number;
  token(): string;
}

const OUTCOMES: readonly NativeHostOutcome[] = ["accepted", "rejected", "unavailable", "uncertain"];

function answer(
  result: SendResult,
  requestId: string,
): { outcome: NativeHostOutcome | "uncertain"; value: Partial<NativeHostResponse> } {
  if (result.kind !== "response") {
    if (result.kind === "error" && /not found|forbidden/i.test(result.message))
      return { outcome: "unavailable", value: {} };
    return { outcome: "uncertain", value: {} };
  }
  const r = result.value as Partial<NativeHostResponse> | null;
  if (
    typeof r !== "object" ||
    r === null ||
    r.v !== 1 ||
    r.request_id !== requestId ||
    !OUTCOMES.includes(r.outcome as NativeHostOutcome)
  )
    return { outcome: "uncertain", value: {} };
  return { outcome: r.outcome as NativeHostOutcome, value: r };
}

/** A settled native outcome as a record state (anything unexpected is uncertain). */
function stateOf(outcome: NativeHostOutcome | "uncertain"): ReconnectState {
  if (outcome === "rejected") return "refused";
  if (outcome === "unavailable") return "unavailable";
  return "uncertain";
}

export class Reconnects {
  private queue: Promise<unknown> = Promise.resolve();
  constructor(private readonly deps: ReconnectDeps) {}

  private update<T>(f: (records: ReconnectRecord[]) => [ReconnectRecord[], T]): Promise<T> {
    const next = this.queue.then(async () => {
      const now = this.deps.now();
      const fresh = (await this.deps.load()).filter((r) => now - r.createdAt <= RECONNECT_TTL_MS);
      const [records, out] = f(fresh);
      await this.deps.save(records.slice(0, MAX_RECORDS));
      return out;
    });
    this.queue = next.catch(() => undefined);
    return next;
  }

  get(id: string): Promise<ReconnectRecord | null> {
    return this.update((records) => [records, records.find((r) => r.id === id) ?? null]);
  }

  /** After the user's action (scope and permission already checked): list the candidates. */
  async begin(
    origin: string,
    storeId: string,
    browser: BrowserKind,
    timeoutMs = RECONNECT_TIMEOUT_MS,
    created?: (record: ReconnectRecord) => unknown,
  ): Promise<ReconnectRecord> {
    const now = this.deps.now();
    const record: ReconnectRecord = {
      id: `r_${this.deps.token()}`,
      origin,
      storeId,
      browser,
      state: "listing",
      candidates: [],
      createdAt: now,
      updatedAt: now,
    };
    await this.update((records) => [[record, ...records], null]);
    await created?.(record);
    const requestId = `req_${this.deps.token()}`;
    const result = await this.deps.send(
      {
        v: 1,
        request_id: requestId,
        op: "reconnect_candidates",
        origin,
        browser,
        profile_id: await this.deps.profileId(),
      },
      timeoutMs,
    );
    const { outcome, value } = answer(result, requestId);
    return this.set(record.id, (r) =>
      outcome === "accepted"
        ? {
            ...r,
            candidates: value.candidates ?? [],
            state: (value.candidates ?? []).length > 0 ? "choosing" : "none",
          }
        : {
            ...r,
            state: stateOf(outcome),
            ...(value.error?.kind ? { code: value.error.kind } : {}),
          },
    );
  }

  /**
   * The user chose `jobId` (and whether to resume). A repeat while sending is ignored; a repeat
   * after an uncertain answer reuses the operation id; another download gets a new one.
   */
  async submit(
    id: string,
    jobId: string,
    resume: boolean,
    timeoutMs = RECONNECT_TIMEOUT_MS,
  ): Promise<ReconnectRecord | null> {
    const claimed = await this.update<ReconnectRecord | null>((records) => {
      const r = records.find((x) => x.id === id);
      const candidate = r?.candidates.find((c) => c.job_id === jobId);
      const open = r && ["choosing", "uncertain", "unavailable", "refused"].includes(r.state);
      if (!r || !candidate || !open) return [records, null];
      const same = r.jobId === jobId && r.resume === resume ? r.operationId : undefined;
      const next: ReconnectRecord = {
        ...r,
        state: "sending",
        jobId,
        resume,
        operationId: same ?? `ext-rc-${this.deps.token()}`,
        updatedAt: this.deps.now(),
      };
      delete next.code;
      return [records.map((x) => (x.id === id ? next : x)), next];
    });
    if (!claimed?.operationId) return claimed;
    const candidate = claimed.candidates.find((c) => c.job_id === jobId);
    if (!candidate) return claimed;
    const s = await this.deps.session(claimed);
    if (!s.ok) return this.set(id, (r) => ({ ...r, state: "refused", code: s.code }));
    const requestId = `req_${this.deps.token()}`;
    const result = await this.deps.send(
      {
        v: 1,
        request_id: requestId,
        op: "reconnect",
        job_id: jobId,
        operation_id: claimed.operationId,
        expected_credential_id: candidate.auth.credential_id,
        session: s.session,
        resume,
      },
      timeoutMs,
    );
    const { outcome, value } = answer(result, requestId);
    return this.set(id, (r) => {
      if (outcome === "accepted" && typeof value.job?.job_id === "string")
        return { ...r, state: "reconnected", phase: value.job.phase };
      if (outcome === "accepted") return { ...r, state: "uncertain" };
      return {
        ...r,
        state: stateOf(outcome),
        ...(value.error?.kind ? { code: value.error.kind } : {}),
      };
    });
  }

  /** A refusal decided before anything was asked (scope, permission, store). */
  refuse(origin: string, code: SignInRefusal): Promise<ReconnectRecord> {
    const now = this.deps.now();
    const record: ReconnectRecord = {
      id: `r_${this.deps.token()}`,
      origin,
      storeId: "",
      browser: "chrome",
      state: "refused",
      code,
      candidates: [],
      createdAt: now,
      updatedAt: now,
    };
    return this.update((records) => [[record, ...records], record]);
  }

  private set(id: string, f: (r: ReconnectRecord) => ReconnectRecord): Promise<ReconnectRecord> {
    return this.update((records) => {
      let out: ReconnectRecord | undefined;
      const next = records.map((r) => {
        if (r.id !== id) return r;
        out = { ...f(r), updatedAt: this.deps.now() };
        return out;
      });
      if (!out) throw new Error("reconnect record disappeared");
      return [next, out];
    });
  }
}
