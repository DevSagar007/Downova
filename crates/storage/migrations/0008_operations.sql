-- B2.16/B2.17 (ADR 0020): durable identities of explicit job operations (Retry, Refresh link).
-- One row per operation that changed a job, written in the same transaction as that change, so a
-- repeated request (double click, lost acknowledgement, service restart) finds the outcome it
-- already had instead of starting another attempt. Pruned with the idempotency records (30 days).
CREATE TABLE operations (
    operation_id       TEXT    PRIMARY KEY NOT NULL
                               CHECK (length(operation_id) BETWEEN 8 AND 128),
    kind               TEXT    NOT NULL CHECK (kind IN ('retry', 'replace_source')),
    job_id             TEXT    NOT NULL REFERENCES jobs (job_id) ON DELETE CASCADE,
    fingerprint_sha256 TEXT    NOT NULL CHECK (length(fingerprint_sha256) = 64),
    outcome            TEXT    NOT NULL CHECK (length(outcome) <= 64),
    created_at_ms      INTEGER NOT NULL CHECK (created_at_ms >= 0)
);
CREATE INDEX operations_by_job ON operations (job_id);
