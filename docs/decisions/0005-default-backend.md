# ADR 0005 — `dm-backend-http` is the default HTTP/HTTPS backend

- **Status:** Accepted
- **Date:** 2026-10-25
- **Supersedes:** the "aria2 stays the default" clauses of ADR 0003 and ADR 0004
- **Evidence:** [HTTP backend report](../reports/b0.4-http-correctness.md) (aria2),
  [dm-backend-http report](../reports/b0.4.2-http-backend.md); CI runs #7, #8, #9

## Context

ADR 0003 gave aria2 a conditional go limited to single-connection plain HTTP. ADR 0004 started a Rust HTTP backend
because aria2 cannot make resume, segmentation, redirect scoping or HTTPS trust safe through its interface. Three
increments of `dm-backend-http` are now complete, each with its acceptance matrix in plain `cargo test`:

| Capability | aria2 1.37.0 (pinned) | dm-backend-http | Evidence for dm-backend-http |
|---|---|---|---|
| `Http` | supported (max-tries 1, one connection) | supported | H01–H06, S01–S02, D01–D05, X01–X05, Z01 |
| `Https` | unverified (WinTLS cannot load a test CA) | supported | T01–T04: fixture CA only as an explicit extra root; self-signed, wrong host and unsupplied CA rejected |
| `ResumeStrongValidator` | unsupported (no If-Range; splices, C01–C06) | supported | V01–V15: strong ETag + length, partial re-verified by SHA-256, exact 206 checks, 200 restarts |
| `SegmentedHttp` | unsupported (C05) | supported | G01–G16: validated 206 per segment, any 200 restarts on one connection, all workers awaited |
| Redirect scoping | headers forwarded cross-origin (D02); http→https bypasses the gate (D04) | credentials/validators origin-scoped; scheme gated per hop | D01–D05, V15 |
| Retry-After | not observable | reported in `BackendError.retry_after` | S02; service waited ≥ 1 s for `Retry-After: 1` |
| Process model | external sidecar, reaped by an exit monitor | in-process, no external program | — |
| Supply chain | libssh2 1.11.0, expat 2.5.0, c-ares 1.19.1 predate fixes (REL-05) | reqwest/hyper/rustls/ring tree, review pending (REL-03) | — |

Cross-platform evidence: the dm-backend-http matrices ran on windows-latest, macos-latest, ubuntu-22.04 and
ubuntu-24.04 in CI #7 (`d0d040b`, first increment), #8 (`4daa6ea`, validated resume) and #9 (`9047a44`, validated
segmentation), all green (public job conclusions; logs not read). Service and CLI integration tests exercised the same
capabilities through `dm-service --backend http` with SHA-256-verified files.

Both backends share one known limit: a server that keeps the same strong ETag for different bytes produces a spliced
file (aria2 C01–C04 even with honest servers; dm-backend-http only with lying servers, V05/G06). Only a trusted SHA-256
detects it.

## Decision

1. **`dm-service` uses `dm-backend-http` when `--backend` is not given.** It needs no external executable, so the default
   service starts on a machine without aria2.
2. **aria2 remains available only through an explicit `--backend aria2`** (optionally with `--aria2 PATH`), with exactly
   its ADR 0003 capability report: `Http` supported, `Https` unverified, `SegmentedHttp` and `ResumeStrongValidator`
   unsupported, credential headers refused.
3. **No fallback and no switching.** The backend is chosen once at startup and fixed for the process. If the chosen
   backend cannot start (for example `--backend aria2` without an aria2c), the service exits with code 4; it never
   starts the other backend. Conflicting or repeated options exit with code 2 and the usage text: `--aria2` without
   `--backend aria2`, `--backend` twice, an unknown backend.
4. **aria2's remaining scope:** a characterisation target and an explicit opt-in engine for plain HTTP. Any future use
   for protocols the HTTP backend does not cover (FTP, BitTorrent, Metalink) needs its own ADR and fixtures.
5. **Release gating:** the pinned aria2 sidecar is **not shipped** while REL-05 is open (dated libssh2/expat/c-ares).
   B0.5 packaging bundles the service and desktop binaries only; the aria2 code path stays compiled in but finds no
   bundled executable, so `--backend aria2` fails honestly in packaged builds unless the user supplies `--aria2 PATH`.

The public IPC contract does not change: `service.v1.hello` already reports the backend id (`"http"`) and its
capability report, which clients use as before.

## Consequences

- Default jobs now use validated resume and up to 4 validated segments per file for large range-capable resources with
  a strong ETag and a length; small or unsuitable resources keep one connection.
- HTTPS works by default with the operating system's trust store (platform verifier). Public HTTPS sites are not part of
  the automated tests (no external network); this is a manual B0.5 check.
- **Still open, unchanged by this decision:**
  - jobs, checkpoints, retries and idempotency are in memory; a service restart loses them (B1, COR-08/COR-09);
  - no connection budget across jobs (CORE-02: ≤ 8 per host, 3 active files) — each file uses at most 4;
  - no speed claims until REL-04 benchmarks;
  - licence and advisory review of the HTTP stack (REL-03) and its update policy;
  - no proxy support (HttpProxy/Socks5/PAC unsupported, never a silent direct connection).
- Tests that exercise aria2 pass `--backend aria2` explicitly; the default-backend tests run without any aria2 on the
  machine.
