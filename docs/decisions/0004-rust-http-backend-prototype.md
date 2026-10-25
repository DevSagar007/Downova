# ADR 0004 — Prototype a Rust HTTP backend behind `DownloadBackend`

- **Status:** Accepted (prototype only; aria2 stays the shipped backend until the prototype passes the same matrix)
- **Date:** 2026-10-25
- **Evidence:** [B0.4 report](../reports/b0.4-http-correctness.md), section "B0.4.1 engine remediation"
- **Related:** ADR 0001 (aria2 first), ADR 0002 (`dm-backend-api`), ADR 0003 (aria2 conditional go)

## Context

ADR 0003 listed what aria2 needs for a full go. B0.4.1 tested the remaining wrapper-level options with the pinned
aria2 1.37.0 (Windows). What was **fixed**:

- Retry and backoff now belong to the service (COR-04): `Retry-After` or bounded jittered backoff, a fresh attempt from
  zero after discarding the partial output. This is proven end to end with aria2 for 503 and a cut connection.
- `--ca-certificate` with a WinTLS build is now refused instead of silently ignored.

What **cannot be fixed** through aria2's existing interface:

1. **Resume and segmentation.** Passing a strong-ETag `If-Match` through `--header` makes a *conforming* server fail with
   412 when the file changes (M02, M03, M07 safe). A server that ignores `If-Match` still produces a spliced file reported
   as complete (M04, unsafe). aria2 never compares the response ETag or sends `If-Range`, so the client cannot detect the
   ignored condition. A HEAD or probe preflight cannot prove that every later request (other node, CDN, retry) evaluates
   the condition.
2. **Diagnostics.** After the first try aria2 reports only "No URI available." and drops the HTTP status (M02, M07), so a
   412, 403 or 5xx on a resumed request cannot be told apart.
3. **Redirect scoping.** aria2 forwards every `--header` (credentials D02, `If-Match` M09) to cross-origin redirect targets
   and follows `http://` → `https://` without a per-hop policy (D04).
4. **Response headers are invisible.** `Retry-After`, `ETag`, `Content-Encoding` and `Content-Disposition` cannot be read
   over RPC, so the service's Retry-After handling only works with backends that report it.
5. **TLS trust is build-specific.** The Windows build uses SChannel and ignores CA files (`WinTLSContext::addTrustedCACertFile`
   returns false), so HTTPS cannot be fixture-tested there without changing the OS trust store. Bundled libraries are
   dated (REL-05).

These are engine-level properties. Patching aria2 means maintaining a C++ fork plus a cross-platform build of its TLS and
library stack.

## Decision

Build a small Rust HTTP backend prototype, `dm-backend-http` (`crates/backend-http`), implementing the existing
`DownloadBackend` trait. Nothing above the trait changes: `dm-service`, the contract and the clients stay as they are,
and the backend is selected at service start.

Stack: Tokio, reqwest (`default-features = false`, `rustls-tls`, `stream`; no cookie store), rustls with the platform
verifier for production roots plus an explicit extra-roots option that tests can use, and SHA-256 via `sha2`.

Correctness rules the prototype must implement and prove:

1. **Validated ranges.** Every ranged request carries `If-Range` with the strong ETag from the first response (weak or
   missing ETag ⇒ no resume and no segmentation for that job). Every response is checked: status 206, an exact
   `Content-Range` for the requested range, the same strong ETag and the same total length. A 200 to a ranged request
   restarts from zero; any mismatch fails with `REMOTE_CHANGED`. This detects a server that ignores conditions,
   which aria2 cannot.
2. **Identity encoding.** `Accept-Encoding: identity`; an encoded response to a ranged request is refused.
3. **Per-hop redirect policy.** Redirects are followed manually. Credentials and validators are dropped on a
   cross-origin hop. Each hop's scheme is gated by the capability report, so `http://` → `https://` cannot bypass the
   `Https` gate.
4. **Errors keep their status.** HTTP status, `Retry-After` (via `dm_domain::retry::parse_retry_after`) and the TLS
   failure reason are reported on every attempt, never a URL.
5. **Own checkpoint format.** A versioned sidecar checkpoint (validator, length, completed ranges), written atomically
   at segment boundaries. It is never shared with aria2 control files.
6. **Bytes go to disk only.** File content never crosses IPC.

Scope: HTTP/1.1 and HTTPS, single and segmented transfers, pause/resume, crash resume from the checkpoint. Out of scope:
FTP, SFTP, BitTorrent, Metalink, proxies (each stays `unsupported` on this backend).

**Promotion rule.** Each capability of the new backend starts `unverified` and is promoted individually, only after the
B0.4 matrix (R, L, C, M, S, D, N, T, K cases) and the service process tests pass against it. HTTPS is promoted only when
T01–T03 pass with the fixture CA supplied as an extra root, which this stack allows on every OS.

## Consequences

- New crate `dm-backend-http`. The dependency table in `docs/project-structure.md` and `ALLOWED_DM_DEPS` in
  `scripts/check-structure.mjs` gain it when the crate is created, with the same rule as aria2: it depends on
  `dm-backend-api` and `dm-domain` only.
- The fixture matrix is made backend-generic so that both backends run the same cases.
- New dependencies (reqwest, rustls, hyper and their tree) need a licence and advisory review (REL-03, REL-05).
- aria2 keeps its ADR 0003 status: `Http` supported, `Https` unverified, `SegmentedHttp` and `ResumeStrongValidator`
  unsupported. If the prototype passes, a later ADR decides the default backend and aria2's remaining role
  (for example FTP/BitTorrent).
- If the prototype fails the matrix, the fallback is the aria2 patch path from ADR 0003 (If-Range, response-ETag check,
  redirect scoping), recorded in a new ADR.
