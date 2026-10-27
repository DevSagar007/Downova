-- B2.3: "Remove from list" (ADR 0013). A removed job keeps its row as a tombstone, so idempotency
-- records (which reference it and cascade on delete) survive: a replay of the original request
-- still finds the original job instead of creating a duplicate. Normal lists omit removed jobs.
-- Ordinary job updates never write this column; only remove_job sets it.
ALTER TABLE jobs ADD COLUMN removed_at_ms INTEGER CHECK (removed_at_ms IS NULL OR removed_at_ms >= 0);
