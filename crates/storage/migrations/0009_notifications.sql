-- CORE-09 (ADR 0022): completion and failure notifications.
-- notify_state: the service's durable decision for a job's terminal outcome.
--   NULL      the job is unfinished (or reopened by Retry/Refresh link): no decision yet;
--   'sent'    a notification was handed to the operating system (at most once);
--   'skipped' no notification (turned off, cancelled by the user, removed, or historical).
-- Jobs that finished before this version are 'skipped': upgrading never notifies the past.
ALTER TABLE jobs ADD COLUMN notify_state TEXT
    CHECK (notify_state IS NULL OR notify_state IN ('sent', 'skipped'));
UPDATE jobs SET notify_state = 'skipped' WHERE phase IN ('completed', 'failed', 'cancelled');
