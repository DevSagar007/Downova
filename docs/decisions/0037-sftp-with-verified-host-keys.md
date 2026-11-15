# ADR 0037 — SFTP with verified host keys

- **Status:** Accepted (2026-11-15). **B2 is not exited.**
- **Requirements:** F05 (FTP and SFTP: "SFTP host keys are verified"), SEC-01, SEC-02
- **Builds on:** [ADR 0035](0035-ftp-in-the-native-engine.md) (FTP in the native engine), [ADR 0036](0036-site-logins-for-ftp.md) (site logins)
- **Evidence:** [PROGRESS.md](../../PROGRESS.md) (2026-11-15 section)

## Context

- **What SFTP is:** a file protocol inside SSH. The whole session is encrypted.
- **Its security rests on the host key:** it is the only thing proving the client reached the real server. A client
  that accepts any key can be read and changed by anyone in the path.
- **Acceptance:** "SFTP host keys are verified."
- **The engine's crypto stays on `ring`:** no aws-lc-rs (ADR 0005, `rustls-no-provider`).

## Decision

**Library:** `russh` 0.65 (Apache-2.0) and `russh-sftp` 3 (Apache-2.0), pure Rust.
- Built with `default-features = false, features = ["ring"]`: no aws-lc-rs, no zlib compression.
- No `rsa` feature: RustCrypto `rsa` has the RUSTSEC-2023-0071 timing advisory. Supported host keys are therefore
  Ed25519 and ECDSA (P-256, P-384, P-521). A server that offers only RSA host keys fails with a clear message.
- **Cost:** about 70 new crates in the lock file, all with permissive licences and recorded in the generated notices.
- **Rejected alternative:** libssh2 (C, needs OpenSSL on Linux).

**Links and sign-in:**
- `sftp://host[:port]/path` (default port 22). As in curl, the path is absolute, and `/~/` means the user's home
  folder.
- SFTP always needs a sign-in. Site logins (ADR 0036) gain the `sftp` scheme (user name and password).
- **No login saved:** the download fails with `AUTH_REQUIRED` and "add a site login for this server".
- Key-based client sign-in is not in this step.

**Host keys** (trust on first use, but only with an explicit decision):
- **Known servers:** the service keeps them in `settings.known_hosts` (host, port, algorithm, SHA-256 fingerprint).
  The keys are public, so they are not secrets.
- **What the engine is given:** `TransferRequest.trusted_host_keys` lists the trusted fingerprints for that server.
- **Comparing a key:** the engine checks the server's key in the SSH handshake, before any password is sent, and
  aborts on a mismatch:
  - no key is known for the server: new error kind `HOST_KEY_UNKNOWN`, next action `verify_server`;
  - a different key is known: new error kind `HOST_KEY_CHANGED`, next action `none`. It is never trusted
    automatically, and the message warns that someone may be intercepting the connection.
  - Both messages carry the key as `ssh-ed25519 SHA256:<base64>`; it is public data. The service records the
    observed key per server as **pending**.
- **Contract:**
  - `known_hosts.v1.list` returns the trusted and pending keys;
  - `known_hosts.v1.trust { host, port, fingerprint }` trusts a key only when it equals the pending one the service
    observed, so a typed or stale value cannot be trusted;
  - `known_hosts.v1.remove { host, port }` forgets a server; a changed key can then be trusted from scratch.
- **Desktop:**
  - a download that failed with `HOST_KEY_UNKNOWN` shows the fingerprint, asks the user to compare it with one from
    the server's administrator, and offers **Trust and retry**;
  - `HOST_KEY_CHANGED` shows a warning and no trust button. Options → Known servers lists the servers and can remove
    one.

**Transfer:**
- **Identity:** `SIZE` and `mtime` from `stat`; the validator is stored as `"sftp-mtime:<seconds>"`, quoted like an
  ETag.
- **Resume:** the read continues at the checkpoint offset only when the size and mtime are unchanged; a changed file
  restarts from zero.
- **Write path:** the same as HTTP and FTP (partial file, checkpoints, disk budget, speed limits, stop). A known size
  must be met exactly.
- **Errors:**
  - a missing file is `LINK_REFRESH_REQUIRED`;
  - a refused sign-in is `AUTH_REQUIRED` ("check the site login"), never including the password;
  - connection failures and timeouts are `NETWORK_OFFLINE`;
  - no shared host-key algorithm is `BACKEND_CAPABILITY_UNAVAILABLE`.

**Capability:** `sftp` becomes `supported` for the native engine after the fixture tests pass. Browsers keep handing
over http(s) only.

## Consequences

- **Not in this step:** RSA-only servers, key-based client sign-in, and SSH agent use.
- **Still open:** FTPS remains a separate step.
- **Binary size:** the shipped binary grows with the SSH crypto. The package smoke jobs report the size.
