-- B1.1: jobs and idempotency records (docs/architecture.md §7, ADR 0006).
-- No segment or byte map: resume state belongs to the engine (ARCH-05).
CREATE TABLE jobs (
    job_id           TEXT    PRIMARY KEY NOT NULL,
    -- Raw source URL (may carry signed-query secrets). Never logged or exported.
    url              TEXT    NOT NULL,
    display_name     TEXT    NOT NULL,
    destination      TEXT    NOT NULL,
    desired_state    TEXT    NOT NULL,
    phase            TEXT    NOT NULL,
    downloaded_bytes INTEGER NOT NULL CHECK (downloaded_bytes >= 0),
    total_bytes      INTEGER CHECK (total_bytes >= 0),
    expected_sha256  TEXT,
    error_json       TEXT,
    created_at_ms    INTEGER NOT NULL,
    updated_at_ms    INTEGER NOT NULL
) STRICT;

CREATE TABLE idempotency_records (
    idempotency_key    TEXT    PRIMARY KEY NOT NULL,
    -- SHA-256 of the canonical request fingerprint (the fingerprint itself contains the URL).
    fingerprint_sha256 TEXT    NOT NULL,
    job_id             TEXT    NOT NULL REFERENCES jobs (job_id) ON DELETE CASCADE,
    created_at_ms      INTEGER NOT NULL
) STRICT;

CREATE INDEX idempotency_records_created ON idempotency_records (created_at_ms);
