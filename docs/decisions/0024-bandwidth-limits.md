# ADR 0024 — Global and per-download bandwidth limits

- **Status:** **Accepted for the B3.1 feature branch** (2026-11-05, implemented on
  `feature/b3.1-speed-limits-schedules` under the narrow sequencing exception recorded in the roadmap; B2 is still not
  exited). Reviewed 2026-11-04 (burst floor, throttled rotation). Corrections found while implementing are in
  "Implementation notes" at the end; they supersede the matching text above them.
- **Date:** 2026-11-04
- **Requirements:** CORE-06 (P0 · B3), ARCH-04 (capabilities), E03 (host budget), CORE-05 (queues),
  COR-01/COR-02 (resume)
- **Builds on:** [ADR 0010](0010-connection-budget-and-admission.md), [ADR 0023](0023-configurable-concurrency-and-request-rotation.md)
  (request permits and rotation), [ADR 0012](0012-exit-and-stop.md) (pause and stop), [ADR 0005](0005-default-backend.md)

## Problem

CORE-06: "Downloads do not saturate the user's connection. Limits are applied and visible in the UI. Measured
throughput stays within ±10% of the limit on the controlled fixture. Limits are gated on the backend reporting the
capability."

Today:
- `Capability::SpeedLimitGlobal` and `SpeedLimitPerJob` exist (`crates/domain/src/capability.rs`);
- `dm-backend-http` reports both `unsupported` ("not implemented");
- `dm-backend-aria2` reports both `unverified` (documented upstream as `max-overall-download-limit` and
  `max-download-limit`);
- no contract, storage, service, CLI or UI support exists.

## Decision

### 1. Units and semantics
- **Canonical unit: bytes per second**, carried as the contract's existing `ByteCount` (an unsigned 64-bit decimal
  string, `#/$defs/ByteCount`), never as a float and never in bits. The UI converts from KB/s or MB/s (1 KB = 1000 B,
  the convention already used for sizes) and shows the converted value back.
- **Global limit:** `{ "bytes_per_second": ByteCount | null }`. `null` means **unlimited**, and it is the default.
- **Per-download limit:** `{ "bytes_per_second": ByteCount | null }`. `null` means **no limit of its own (inherits
  only the global limit)**. A download runs at most at the smaller of the two.
- **There is no separate "unset" state:** `null` is the only way to say "no limit", and absent fields in a request are
  refused (closed objects, as in ADR 0023).
- **Bounds:**
  - minimum **1,024 B/s**: smaller values would make the burst rule below dominate and the ±10 % target untestable;
  - maximum **10^12 B/s** (1 TB/s): larger values are refused rather than silently meaning "unlimited";
  - `0` is refused: it is not a synonym for unlimited or for pause, which has its own command.

### 2. Enforcement (in `dm-backend-http`)
- **Token buckets:** a new module `crates/backend-http/src/rate.rs` with an injectable clock (`trait Clock`; tokio time
  in production, a manual clock in tests). There are two kinds of bucket:
  - **one global bucket**, shared by every attempt of the backend;
  - **one bucket per download (attempt)**, shared by all of its segments, its probe body and every redirect hop. A
    segment never owns a budget: four segments split one file budget.
- **Charging:** every body chunk is charged to the per-download bucket, then to the global one, **before** it is
  written. This is the `bytes_stream()` loop in `crates/backend-http/src/transfer.rs`, which segmented transfers also
  use. When both are unlimited, the path costs one check and no wait.
- **Bounded burst (revised in review, 2026-11-04):**
  - **The capacity is `max(rate × 250 ms, 1 KiB)`.**
    - From 4,096 B/s up, the longest burst after idling is exactly 250 ms of the rate.
    - Below 4,096 B/s the 1 KiB floor applies. At the 1,024 B/s minimum that is 1 s; it is never more.
  - **Why it changed:** the first draft's `max(rate × 250 ms, 16 KiB)` contradicted its own "quarter-second" claim:
    at 1,024 B/s a 16 KiB floor allows a 16 s burst.
  - **Large chunks:** a chunk larger than the capacity is **charged in capacity-sized parts**, and the writer waits
    for each part. A 64 KiB chunk under a 16 KiB/s limit (capacity 4 KiB) takes about 4 s and never overdraws.
  - **No debt** is carried beyond one capacity.
  - **Both buckets apply together:** each part is taken from the per-download bucket and then from the global one, in
    that order, and never pre-reserved from one while waiting on the other. The rate seen on the wire is therefore at
    most the smaller of the two, with no overshoot.
- **Waits are cancellation-safe:** a wait is a `select!` between the refill deadline and the attempt's cancellation
  token, so pause, remove, cancel and service shutdown end it at once. Nothing is written after cancellation.
- **Waiting is not a network timeout:** the per-read timeout (`HttpConfig.read_timeout`) runs only while the
  engine waits for the network, never while it waits for tokens, so a low limit never turns into a spurious
  `NetworkOffline`. The server sees ordinary TCP back-pressure. (Corrected in implementation: reqwest's client
  `read_timeout` cannot provide this; see "Implementation notes".)
- **Live changes:**
  - a new rate replaces the bucket's rate atomically, and the tokens are clamped to the new capacity;
  - waiting writers recompute their deadline on a change notification (`tokio::sync::watch`) and do not sleep out the
    old deadline;
  - no request is restarted or reopened: the transfer continues on the same connection.

### 3. Interaction with existing rules
- **Request permits (ADR 0010/0023):** a throttled request keeps its host permit, so throttling does not open more
  connections.
- **Rotation (revised in review, 2026-11-04):**
  - **Problem:** rotation is byte-based today. `ROTATION_BYTES` = 1 MiB in `crates/backend-http/src/segmented.rs`
    yields a segment's permit to waiters after 1 MiB of further body. At a 2 KB/s per-download limit that is about
    512 s holding a host permit while another file waits for its probe: the fairness ADR 0023 gives would be lost
    under throttling.
  - **Decision:** a validated range segment that sees waiters yields after **1 MiB or 2 s of holding the permit,
    whichever comes first**, measured from the start of its turn. The 2 s include time spent waiting for tokens.
    The yield path is ADR 0023's: flush, sync, publish the segment's checkpoint, drop the response, then rejoin the
    FIFO for the remaining range with full revalidation.
  - **Unchanged:** without waiters nothing changes (no extra handshakes). A **non-range stream** (`?norange`, a
    weak ETag) still never rotates: it cannot resume a range safely, so it keeps its permit until it ends, exactly
    as today.
  - **Proof required:** a test where a 2 KB/s download holds the host's last permit and a second file's probe gets
    one within about 2 s.
- **Segments:** splitting decisions are unchanged. The per-download bucket makes extra segments useless for exceeding
  the limit.
- **Pause and resume (ADR 0012):** pausing drops the attempt's bucket. Resuming starts a fresh bucket at the job's
  current limit with an empty burst, so a resume never gets a burst on top of the limit.
- **Restart and recovery:** limits are durable (§4). Recovered attempts get their limits before their first body
  byte.
- **Queues and admission:** independent. A limit never changes how many downloads are admitted.

### 4. Durability (in `dm-service` and `dm-storage`)
- **Global limit:** in the existing `settings` table under its own key (`speed_limit_global_bps`), as
  `max_active_downloads` is; no migration.
- **Per-download limit:** a nullable column on jobs, **migration `0010_speed_limits.sql`**, with reopen tests and a
  test that older databases read as `null`.
- **Persist, then apply:**
  - the service validates, checks the capability, writes, and only then changes the engine's effective rate and
    acknowledges;
  - a refused write changes neither the stored nor the effective limit (the same trigger-based test as B2.31's
    concurrency save);
  - if applying to a running attempt fails after the write, the stored value stays and the attempt is restarted from
    its checkpoint with the stored limit; the result reports this.
  - Clients never retry a set automatically after a lost acknowledgement; they read the value back (ADR 0023 rule).

### 5. Capabilities
- `set` with a non-null limit is refused with `BACKEND_CAPABILITY_UNAVAILABLE` unless the active backend reports the
  matching capability `supported`. `null` (unlimited) is always accepted.
- `dm-backend-http` turns `SpeedLimitGlobal` and `SpeedLimitPerJob` `supported` **only in the change that adds the
  throughput fixture tests (§7)**.
- **aria2 (optional, ADR 0003):** both stay `unverified`, so limits are refused there, until fixture tests prove
  `max-overall-download-limit` (global, `changeGlobalOption`) and `max-download-limit` (per GID, `changeOption`).
  aria2 GIDs stay inside its adapter.

### 6. Surfaces

| Layer | Change |
|---|---|
| Contract (`packages/contracts/schema/service-api.v1.schema.json`, generated TS, `crates/domain/src/api.rs`) | `speed_limits.v1.get` / `speed_limits.v1.set` `{ global_bytes_per_second: ByteCount \| null }`; `downloads.v1.set_speed_limit` `{ job_id, bytes_per_second: ByteCount \| null }`; optional `speed_limit_bytes_per_second` on `downloads.v1.add`; the job snapshot gains `speed_limit_bytes_per_second`; examples and the closed method set |
| `crates/domain` | bounds validation; no I/O |
| `crates/backend-api` | `set_global_rate(Option<u64>)` and `set_rate(&BackendHandle, Option<u64>)`, both returning `BACKEND_CAPABILITY_UNAVAILABLE` by default |
| `crates/backend-http` | `rate.rs`, wiring in `transfer.rs` and `segmented.rs`, capabilities |
| `crates/backend-aria2` | option mapping, still `unverified` |
| `crates/storage` | the settings key; migration 0010 |
| `crates/service` | persist-then-apply, restore before recovery, refusal on capability |
| `crates/cli` | `dm speed-limit`, `dm speed-limit-set <N\|off>`, `dm job-speed-limit <JOB-ID> <N\|off>`, `dm add --speed-limit <N>`; accepts `500k`, `2m` (×1000) and plain bytes/s |
| `apps/desktop` | Options → Downloads: a global limit (Unlimited / value with a unit); the details panel: this download's limit; both disabled, with the backend's reason, when the capability is not `supported`; every string in `src/i18n/messages.ts` in English and Bangla |
| `docs` | architecture §, testing §, traceability, CORE-06 status |

## Verification
- **Deterministic `rate.rs` tests** with a manual clock:
  - the steady rate; the burst bound; chunks larger than the capacity;
  - live increases and decreases mid-wait; unlimited to limited and back;
  - cancellation during a wait; many writers sharing one bucket.
- **Backend integration** on the controlled fixture (`scripts/perf/fixture-process.mjs` and the Rust fixture):
  - one download with 4 segments under a per-download limit;
  - 1, 3 and 6 downloads under one global limit;
  - mixed global and per-download limits;
  - the steady-state throughput, measured over a window after the first second from the fixture's bytes written and
    the job's bytes, must be **within ±10 %**.
- **Live change:** halving and then doubling the limit mid-transfer reaches the new rate within one second, with no
  new request starting from 0 (the fixture's request log).
- **Service:**
  - persistence across a restart; a refused write changes nothing;
  - `BACKEND_CAPABILITY_UNAVAILABLE` with the backend reporting `unsupported` or `unverified`;
  - `null` is always accepted.
- **Rotation under throttling:** a 2 KB/s download holding the host's last permit lets a waiting file's probe
  start within about 2 s, and the resumed range revalidates (status, `Content-Range`, ETag).
- **Both budgets together:** a per-download limit above the global one, and the opposite, each measured within
  ±10 % of the smaller.
- **No spurious timeouts:** a 2 KB/s limit for 2 minutes with `read_timeout` at its production value completes, with
  no `NetworkOffline`.
- **CLI and UI:** argument tests; desktop tests in both languages, including the disabled state with the reason.
- **Packaged acceptance:** a row in the concurrency acceptance style: set limits in Options, measure within ±10 %,
  restart, and the limits remain.

## Consequences
- **Gains:** limits are exact, shared and live, and never cost a connection or a restart.
- **Costs:** one extra check per body chunk when unlimited; a migration; a backend-trait addition.
- **Rejected alternatives:**
  - OS-level shaping: not portable, and it would need privileges;
  - limiting by delaying socket reads only: it would not share a budget across segments;
  - limits in the UI: they would violate the service-owns-transfers rule.

## Implementation notes (B3.1, 2026-11-05)

- **Read timeout (correction, real defect found by test S05):** reqwest's client-level `read_timeout` keeps running
  while the body is not polled, so time spent waiting for tokens counted as network silence: at 1 KiB/s with a 1 s
  timeout the download failed with `NetworkOffline`. The client no longer sets it. The engine owns the per-read
  timeout instead (`transfer::within`), wrapped around each request send and each body read only; token waits are
  outside it. The same applies to segmented transfers and source replacement.
- **Where:** `crates/backend-http/src/rate.rs` (buckets: capacity `max(rate/4, 1 KiB)`, rate as an atomic with 0 =
  unlimited, a `watch` channel for live changes, waits cancelled by dropping the future); `Rates::charge` takes the
  job bucket, then the global one. Writes are split into parts no larger than the smallest capacity.
- **Rotation:** a throttled segment yields its connection after 1 MiB or 2 s (`ROTATION_TIME`) when others wait.
- **Contract:** `speed_limits.v1.get/set` (`{ global_bytes_per_second: ByteCount | null }`, required, nullable),
  `downloads.v1.set_speed_limit` (`{ job_id, bytes_per_second }`), `AddDownloadParams.speed_limit_bytes_per_second`
  and `DownloadSnapshot.speed_limit_bytes_per_second`. Values are decimal strings from 1024 to 10^12; `0` is refused
  (it is not pause). The schema pattern enforces the bounds.
- **Storage:** migration 10 (`jobs.speed_limit_bps` with a CHECK; the global value in `settings`). Restored before
  recovery: the global rate is applied to the backend first, and recovered attempts carry their own limit.
- **Idempotency:** a request without a limit keeps its old fingerprint; one with a limit gets a separate element, so a
  replay with a different limit is a conflict.
- **A limit on a finished download is refused;** on a download with no running attempt it applies to the next one.
- **aria2:** both capabilities stay `unverified`; `start` with a limit is refused with
  `BACKEND_CAPABILITY_UNAVAILABLE` (no fixture demonstrates aria2's behaviour).
- **Evidence:** see [the B3.1 report](../reports/b3.1-speed-limits-schedules.md).

### Stabilization notes (B3.1, second batch)

- **S07:** a genuinely stalled server still times out under a limit: no headers after 1.03 s, a mid-body stall after
  about 3 s (1 s timeout, 2 KB/s limit).
- **No lock is held during a token wait.** The bucket and segment mutexes are scoped; the coordinator and the store
  are not involved.
- **The per-download control in the details:**
  - it is guarded against a double submit with a ref;
  - it is keyed by job, so a late answer never shows on another download.
- **Packaged GUI acceptance (Windows):** the Options and details saves through the real window, with client steady
  state −2.3 % (400 KB/s overall) and −2.7 % (200 KB/s per download). The server-side count is a bounded
  cross-check only: with several range connections, socket buffers separate it from what the client received.
