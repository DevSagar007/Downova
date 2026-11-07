-- SEC-01/SEC-02/BRW-05 (ADR 0027).
--   credentials            login contexts by opaque id: origin, browser, profile id, mode,
--                          generation, state and earliest expiry. Never a cookie or token: a
--                          remembered context's secret lives only in the OS credential store,
--                          a temporary one only in the service's memory.
--   jobs.credential_id     the context a signed-in download uses (NULL: public).
--   jobs.url_sealed        the full URL, AES-256-GCM under the data key held in the OS
--                          credential store, when it carries protected material (userinfo, a
--                          query, a fragment, or a signed-in job); jobs.url then holds only the
--                          display form. NULL: nothing protected, or no key (memory only).
--   resume_checkpoints.sealed  1: payload is sealed the same way (it holds the final URL).
-- Existing rows are re-sealed by the service once it has the key (one row per transaction).
CREATE TABLE credentials (
    credential_id TEXT    PRIMARY KEY NOT NULL CHECK (length(credential_id) = 21 AND credential_id GLOB 'cred_*'),
    origin        TEXT    NOT NULL CHECK (length(origin) <= 300),
    browser       TEXT    NOT NULL CHECK (browser IN ('chrome', 'edge')),
    profile_id    TEXT    NOT NULL CHECK (length(profile_id) BETWEEN 16 AND 64),
    mode          TEXT    NOT NULL CHECK (mode IN ('temporary', 'remembered')),
    state         TEXT    NOT NULL CHECK (state IN ('pending', 'active', 'forgotten')),
    generation    INTEGER NOT NULL DEFAULT 1 CHECK (generation >= 1),
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER
) STRICT;
ALTER TABLE jobs ADD COLUMN credential_id TEXT;
ALTER TABLE jobs ADD COLUMN url_sealed BLOB;
ALTER TABLE resume_checkpoints ADD COLUMN sealed INTEGER NOT NULL DEFAULT 0 CHECK (sealed IN (0, 1));
