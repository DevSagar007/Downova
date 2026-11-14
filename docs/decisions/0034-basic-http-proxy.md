# ADR 0034 — Basic HTTP proxy in the native engine

- **Status:** Accepted (2026-11-14). **B2 is not exited.**
- **Requirements:** F06 (basic HTTP proxy, P1 · B4), SEC-01, SEC-02, ARCH-04
- **Amends:** [ADR 0005](0005-default-backend.md) ("no proxy support"): the native engine now supports one
  HTTP proxy. SOCKS5, PAC, the system proxy, Digest, NTLM and Kerberos stay unsupported (B6, spike first).
- **Evidence:** [PROGRESS.md](../../PROGRESS.md) (2026-11-14 section)

## Context

- Users behind a corporate or ISP proxy cannot download without one.
- Acceptance (F06):
  - a profile shows which protocols it carries;
  - an unsupported profile never falls back to a direct connection;
  - passwords never reach SQLite, output or logs.
- ADR 0027 already gives the service an OS credential store (`dm_platform::vault`) with bounded calls (ADR 0032).

## Decision

**One proxy setting, owned by the service.**
- **Mode:** `direct` (the default) or `http`.
- **Fields:** host, port, an optional user name, and an optional password.
- **Kept when switching to direct:** the fields stay saved, so turning the proxy back on needs no retyping.

**Contract** (`proxy.v1.get`, `proxy.v1.set`):
- `ProxySettings` (the get result) holds `mode`, `host`, `port`, `username`, `password_state`
  (`none` | `saved` | `unavailable`) and `carries`.
- `carries` lists the download protocols the profile carries: `["http", "https"]`, with HTTPS through `CONNECT`.
  Anything else (FTP, SFTP) is not offered by the engine at all.
- `SetProxyParams` holds `mode`, `host`, `port`, `username` and `password`:
  - `password` absent: keep the saved one;
  - `null`: remove it;
  - a string: replace it.
- The password is a `SecretTransit` on the way in and is never returned.

**Storage:**
- SQLite `settings` holds `proxy` = mode, host, port, user name and whether a password is saved, as JSON.
- The password itself is only in the OS credential store, under the id `proxy-password`.

**Save order:**
1. Validate.
2. Check the capability.
3. Write the password to the store (a store that refuses or does not answer fails the save and changes nothing).
4. Write the row.
5. Apply to the engine.

The settings answered by a get are never ahead of what is durable.

**Engine** (`dm-backend-http`):
- `DownloadBackend::set_proxy(Option<ProxyConfig>)`. The default implementation refuses with
  `BACKEND_CAPABILITY_UNAVAILABLE`, so aria2 and the fakes do not silently ignore a proxy.
- The native engine rebuilds its `reqwest` client with `Proxy::all(http://host:port)` and Basic credentials. The
  password is held as a `Secret`.
- **No fallback:** a client with a proxy never tries a direct connection.
- **Which transfers use it:** transfers started or resumed after a save use the new client. A running transfer
  keeps its open connection until it is paused, resumed or retried.
- **Capability:** `HttpProxy` becomes `supported` for the native engine, with the note "HTTP proxy for http and
  https (CONNECT), Basic sign-in; no system proxy, SOCKS5 or PAC".
- **Errors:**
  - 407 (a response, or a refused `CONNECT`) is `AUTH_REQUIRED`;
  - a refused tunnel is `NETWORK_OFFLINE` with "the proxy refused to connect to the server";
  - a proxy that cannot be reached is `NETWORK_OFFLINE`.

**Never direct by accident.** The service refuses to start or resume a transfer when the saved proxy is not in
effect:
- the engine cannot use a proxy (for example `--backend aria2` with a saved HTTP proxy):
  `BACKEND_CAPABILITY_UNAVAILABLE`;
- the saved password cannot be read now (a locked or unanswering store): `CREDENTIAL_STORE_UNAVAILABLE`.
  - Each start retries one bounded read.
  - Saving the settings again also clears the block.

**Secrets:**
- `ProxyConfig`, the settings row and every error keep the password out of `Debug`, logs, events, diagnostics
  and exports.
- The proxy URL handed to `reqwest` never carries credentials.
- The desktop form holds a typed password only until it is sent, and shows only whether one is saved.

## Consequences

- **Clients:**
  - the desktop has Options → Proxy;
  - the CLI does not have a proxy command yet; it is a documented gap.
- **Not covered:** per-site exceptions (no-proxy lists), several profiles, and the system proxy (B6).
- **macOS/Linux:** use the same code path and store, but are verified only where a check ran.
