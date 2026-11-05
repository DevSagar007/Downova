-- F07/E06 (ADR 0026): the attachment check of a finished download.
--   jobs.scan_json                       the durable scan evidence (status, reason, origin
--                                        marking) as JSON; NULL: none recorded. Jobs finished
--                                        before this migration keep NULL, so they are never
--                                        presented as checked.
--   finalization_journal.scan_started_ms the check of this placement started (committed before the
--                                        OS call); NULL: not started. A sub-state of 'placed', so
--                                        the stage CHECK constraint is unchanged.
--   finalization_journal.final_file_id   the placed file's identity (volume and file index, or
--                                        device and inode), recorded with 'placed': a different
--                                        file at the path never inherits this verification.
ALTER TABLE jobs ADD COLUMN scan_json TEXT;
ALTER TABLE finalization_journal ADD COLUMN scan_started_ms INTEGER CHECK (scan_started_ms >= 0);
ALTER TABLE finalization_journal ADD COLUMN final_file_id TEXT;
