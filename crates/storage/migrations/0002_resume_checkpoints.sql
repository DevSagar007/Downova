-- B1.2: durable resume checkpoints (ADR 0007).
-- The payload is opaque to the service: segment layout, validators and digests are encoded and
-- interpreted only by the backend that wrote it. There is still no service-side segment map.
ALTER TABLE jobs ADD COLUMN attempt_id TEXT;

CREATE TABLE resume_checkpoints (
    job_id          TEXT    PRIMARY KEY NOT NULL REFERENCES jobs (job_id) ON DELETE CASCADE,
    -- The attempt that produced the checkpoint; it must be the job's current attempt.
    attempt_id      TEXT    NOT NULL,
    backend_id      TEXT    NOT NULL,
    backend_version TEXT,
    format_version  INTEGER NOT NULL CHECK (format_version >= 0),
    confirmed_bytes INTEGER NOT NULL CHECK (confirmed_bytes >= 0),
    payload         BLOB    NOT NULL CHECK (length(payload) <= 65536),
    updated_at_ms   INTEGER NOT NULL
) STRICT;
