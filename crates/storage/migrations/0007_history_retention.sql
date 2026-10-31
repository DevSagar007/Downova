-- B2.12 (ADR 0018): history retention.
-- finished_at_ms: when the job first became completed, failed or cancelled. Written once by the
-- storage layer at the first terminal write and never changed afterwards (renames, queue moves,
-- settings and removal do not touch it).
ALTER TABLE jobs ADD COLUMN finished_at_ms INTEGER CHECK (finished_at_ms IS NULL OR finished_at_ms >= 0);

-- Jobs that finished before this version have no recorded finish time. They count from this
-- upgrade (conservative: a retention period can never remove them earlier than it would a job
-- finished today).
UPDATE jobs SET finished_at_ms = CAST(strftime('%s', 'now') AS INTEGER) * 1000
WHERE phase IN ('completed', 'failed', 'cancelled');
