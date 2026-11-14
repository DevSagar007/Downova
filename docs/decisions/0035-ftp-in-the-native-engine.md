# ADR 0035 — Anonymous FTP in the native engine

- **Status:** Accepted (2026-11-14). **B2 is not exited.**
- **Requirements:** F05 (FTP and SFTP, P1 · B4), ARCH-04, CORE-04
- **Amends:** [ADR 0005](0005-default-backend.md): the native engine (`dm-backend-http`) now also carries `ftp://`.
- **Evidence:** [PROGRESS.md](../../PROGRESS.md) (2026-11-14 section)

## Context

- F05 asks for FTP and SFTP downloads with:
  - protocol-specific login, retry and resume fixtures;
  - verified SFTP host keys;
  - `supported` reported only after the fixtures pass.
- Links with an embedded user name or password are refused everywhere (`check_url`), so a password never travels or
  sits in a stored URL. FTP sign-in therefore cannot use `ftp://user:pass@host`.
- The engine already has one single-connection write path that owns the partial file, the checkpoint (offset plus
  SHA-256 state), disk budgets and speed limits.

## Decision

**Scope of this step: anonymous FTP only.**
- Each step can be released on its own.
- Next: FTP with a user name and password (kept in the OS credential store and bound to the job, like signed-in
  downloads) and SFTP with host-key verification.
- FTPS: separate.

**Library:** `suppaftp` 12 (MIT OR Apache-2.0) with its tokio runtime, without TLS features.
- It brings `lazy-regex`, `pin-project` and `thiserror` 2 into the shipped closure. They are recorded in the
  generated notices.

**Transfer:**
- Login is `anonymous` / `anonymous@`.
- Binary type.
- **Passive data only.** EPSV is tried first, then PASV. The data connection always goes to the control connection's
  peer address (the "NAT workaround"): an address in a PASV reply is never trusted, which closes FTP bounce and spoofed
  data-address attacks. Active mode is never used.
- **File identity:** the length is `SIZE`. The validator is `MDTM`, kept in the identity's validator field as
  `"ftp-mdtm:<timestamp>"`. It is quoted like a strong ETag, so the durable checkpoint format is unchanged.
- **Resume:** `REST <offset>` + `RETR`, only when `SIZE` and `MDTM` both match the checkpoint.
  - A different file restarts from zero (like an HTTP `200` to a range request).
  - A server without `MDTM` cannot resume (`BACKEND_CAPABILITY_UNAVAILABLE`, the same rule as HTTP without a strong
    ETag).
- **Completion:** the bytes go through the same write path as HTTP (partial file, checkpoints, disk budget, speed
  limits, stop).
  - The transfer counts as complete only after the server's final `226`/`250` reply.
  - A known `SIZE` must also be met exactly.
- **One connection per file:** no segmented FTP. The per-host connection budget applies to the control connection.

**Errors:**

| Server answer | Error kind | Notes |
|---|---|---|
| `530` (login refused) | `AUTH_REQUIRED` | the server needs a sign-in, which a later step adds |
| `550` (no such file) | the HTTP 404 classification | |
| `421`, `425`, `426`, `450`, `451`, a timeout or a connection failure | `NETWORK_OFFLINE` | retried like HTTP |
| other `5xx` | `INVALID_REQUEST` | the server's text is never echoed |

**Service and clients:**
- `downloads.v1.add` accepts `ftp://` URLs (contract pattern `^(https?|ftp)://`). The service gates them on the `ftp`
  capability.
- `check_download_url` (desktop and service) accepts `ftp://`.
- The browser host's `check_url` stays http(s) only: browsers hand over their own downloads.
- A user name or password in the link is still refused everywhere.

**Capability:** `ftp` becomes `supported` for the native engine once the fixture tests pass. The note says
"anonymous, passive, no FTPS". `sftp` stays `unsupported`.

## Consequences

- **Plain FTP:** the protocol is unencrypted. Anonymous FTP sends no secret, but the file can be read or changed in
  transit.
  - The UI says so: the link field explains it, and the details show the protocol.
  - A published checksum (Add URL → SHA-256) is the way to detect tampering.
- **Next steps:** FTP sign-in and SFTP are the remaining F05 work. F05 stays `in-progress` until they exist.
