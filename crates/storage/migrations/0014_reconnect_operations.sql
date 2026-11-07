-- BRW-05 reconnect (ADR 0028): `operations` also records reconnects, so a repeated reconnect
-- (a lost acknowledgement) is answered from the record and changes nothing twice. SQLite cannot
-- widen a CHECK constraint in place, so the table is rebuilt with every row kept. Nothing else
-- references `operations`.
CREATE TABLE operations_v14 (
    operation_id       TEXT    PRIMARY KEY NOT NULL
                               CHECK (length(operation_id) BETWEEN 8 AND 128),
    kind               TEXT    NOT NULL CHECK (kind IN ('retry', 'replace_source', 'reconnect')),
    job_id             TEXT    NOT NULL REFERENCES jobs (job_id) ON DELETE CASCADE,
    fingerprint_sha256 TEXT    NOT NULL CHECK (length(fingerprint_sha256) = 64),
    outcome            TEXT    NOT NULL CHECK (length(outcome) <= 64),
    created_at_ms      INTEGER NOT NULL CHECK (created_at_ms >= 0)
);
INSERT INTO operations_v14 (operation_id, kind, job_id, fingerprint_sha256, outcome, created_at_ms)
    SELECT operation_id, kind, job_id, fingerprint_sha256, outcome, created_at_ms FROM operations;
DROP TABLE operations;
ALTER TABLE operations_v14 RENAME TO operations;
CREATE INDEX operations_by_job ON operations (job_id);
