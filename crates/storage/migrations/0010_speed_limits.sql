-- CORE-06 (ADR 0024): a download's own bandwidth limit, bytes per second.
-- NULL: no limit of its own (only the global limit applies). Every existing job reads as NULL,
-- so upgrading changes no transfer. The global limit is a settings row
-- ('speed_limit_global_bps'), absent = unlimited, like 'max_active_downloads'.
-- Ordinary job writes list their columns explicitly and never touch this one; it changes only
-- through Store::set_job_speed_limit or together with the job's first insert.
ALTER TABLE jobs ADD COLUMN speed_limit_bps INTEGER
    CHECK (speed_limit_bps IS NULL OR (speed_limit_bps >= 1024 AND speed_limit_bps <= 1000000000000));
