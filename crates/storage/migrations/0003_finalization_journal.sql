-- B1.3: completion journal (ADR 0008). One row per job while it is being finalized:
-- intent -> verified -> placed -> complete. The row binds the stage to the job's attempt and to the
-- staging file that attempt owns, so recovery after a crash knows exactly which files are Downova's.
CREATE TABLE finalization_journal (
    job_id        TEXT    PRIMARY KEY NOT NULL REFERENCES jobs (job_id) ON DELETE CASCADE,
    attempt_id    TEXT    NOT NULL,
    stage         TEXT    NOT NULL CHECK (stage IN ('intent', 'verified', 'placed', 'complete')),
    -- 'staged': the backend left a staging file the service places; 'engine': the engine wrote
    -- the destination itself (aria2) and verification happens in place.
    placement     TEXT    NOT NULL CHECK (placement IN ('staged', 'engine')),
    staging_path  TEXT    NOT NULL,
    final_path    TEXT    NOT NULL,
    length        INTEGER CHECK (length >= 0),
    sha256        TEXT    CHECK (sha256 IS NULL OR length(sha256) = 64),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

-- Verification evidence of a completed job: the SHA-256 of the final file, and whether it matched
-- a trusted publisher checksum ('publisher_checksum') or was only computed ('local_hash').
ALTER TABLE jobs ADD COLUMN verified_sha256 TEXT;
ALTER TABLE jobs ADD COLUMN verification_source TEXT;
