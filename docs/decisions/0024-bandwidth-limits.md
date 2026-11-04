# ADR 0024 — Global and per-download bandwidth limits

- **Status:** **Proposed.** Implementation starts only after B2 exits (roadmap: B3 needs B2). This design is prepared
  while the remaining B2 evidence is collected.
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
- **Bounded burst:**
  - the capacity is `max(rate × 250 ms, 16 KiB)`, so a long idle period never allows more than a quarter-second burst;
  - a chunk larger than the capacity is **charged in capacity-sized parts**: the writer waits for each part, so a
    64 KiB chunk under a 16 KiB/s limit takes about 4 s and never overdraws;
  - no debt is carried beyond one capacity.
- **Waits are cancellation-safe:** a wait is a `select!` between the refill deadline and the attempt's cancellation
  token, so pause, remove, cancel and service shutdown end it at once. Nothing is written after cancellation.
- **Waiting is not a network timeout:** the per-read `read_timeout` (`HttpConfig.read_timeout`) runs only while the
  engine waits for the network. It is paused while the engine waits for tokens, so a low limit never turns into a
  spurious `NetworkOffline`. The server sees ordinary TCP back-pressure.
- **Live changes:**
  - a new rate replaces the bucket's rate atomically, and the tokens are clamped to the new capacity;
  - waiting writers recompute their deadline on a change notification (`tokio::sync::watch`) and do not sleep out the
    old deadline;
  - no request is restarted or reopened: the transfer continues on the same connection.

### 3. Interaction with existing rules
- **Request permits (ADR 0010/0023):** a throttled request keeps its host permit, so throttling does not open more
  connections. Under contention, the 1 MiB rotation still applies: a slow segment yields its permit after 1 MiB as
  today, so a throttled download cannot starve other files' probes.
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
