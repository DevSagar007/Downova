# ADR 0036 — Site logins for FTP

- **Status:** Accepted (2026-11-15). **B2 is not exited.**
- **Requirements:** F05 (FTP login), SEC-01, SEC-02; IDM parity (Options → Site logins)
- **Builds on:** [ADR 0035](0035-ftp-in-the-native-engine.md) (anonymous FTP), [ADR 0027](0027-secret-lifecycle-vault-and-authenticated-get.md) (OS credential store), [ADR 0032](0032-bounded-credential-store-calls.md) (bounded store calls), [ADR 0034](0034-basic-http-proxy.md) (the same pattern for the proxy password)
- **Evidence:** [PROGRESS.md](../../PROGRESS.md) (2026-11-15 section)

## Context

- **The problem:** FTP servers that are not anonymous need a user name and password. Links never carry them: `user:pass@` is refused everywhere.
- **Browser sign-ins (ADR 0027) do not fit:** they are scoped browser sessions (cookies, a browser, a profile). An FTP login is a plain user name and password for one server.
- **The model to copy:** Internet Download Manager keeps a "Site logins" list: one entry per server, used for every download from it.

## Decision

**Site logins:**
- One login per FTP server, keyed by `ftp`, host (case-insensitive) and port (default 21). Up to 100 logins.
- The service owns them.
- **Contract:**
  - `site_logins.v1.list` returns `SiteLogin { login_id, scheme, host, port, username, password_state }`;
  - `site_logins.v1.set` takes `SetSiteLoginParams { scheme, host, port, username, password? }` and adds or replaces the
    login for that server. `password` absent keeps the saved one; a string replaces it;
  - `site_logins.v1.remove` takes `{ login_id }`.
- The password is a `SecretTransit` on the way in and is never returned.

**Storage:**
- SQLite `settings.site_logins` holds the list without passwords (JSON).
- Each password is only in the OS credential store under `site-login-<login_id>`.
- **Save order:** validate, write the password to the store (bounded; a refusal changes nothing), save the row.
- **Remove order:** the row goes first, then the stored password.

**Use:**
- The service keeps the passwords it could read in memory: read at startup (bounded) and on every save.
- When an `ftp://` transfer starts, resumes or is restored, the service puts the matching login into
  `TransferRequest.login`. With no matching login, the transfer is anonymous (ADR 0035).
- **A password the store cannot give now** holds the transfer with `CREDENTIAL_STORE_UNAVAILABLE`; it never falls back
  to anonymous with a stored login.
  - The same re-read as the proxy (ADR 0034): at add, resume, retry and list, outside the coordinator lock.

**Engine:**
- `TransferRequest.login: Option<Login { username, password: Secret }>`. `Debug` is redacted.
- The FTP transport signs in with it instead of `anonymous`. HTTP ignores it (HTTP site logins are a later step).
- A refused sign-in (530) is `AUTH_REQUIRED`: "the FTP server refused the sign-in; check the site login".
- Plain FTP sends the password unencrypted. The UI says so where a login is saved.

**Desktop:**
- Options → Site logins: the list (server, user name, password saved or unavailable, Remove).
- An add form: server, port, user name, password. The password is write-only, kept only until it is sent, and cleared
  afterwards.

## Consequences

- **Existing behaviour:** links with credentials stay refused; anonymous FTP is unchanged when no login matches.
- **Next steps:** FTPS and SFTP (with host-key verification), then site logins for HTTP Basic.
