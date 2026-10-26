-- B1.6: file naming (ADR 0011).
-- staging_name: fixed when the job is accepted; the backend's partial and control files are named
--   after it, and checkpoints bind to it. It never changes.
-- display_name: the final name. Provisional while name_source is NULL; frozen (never changed again)
--   once name_source is set.
ALTER TABLE jobs ADD COLUMN staging_name TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN name_source TEXT CHECK (name_source IS NULL OR name_source IN
    ('filename_ext', 'filename', 'response_url', 'request_url', 'default', 'legacy'));

-- Jobs accepted before B1.6 used one name for the partial file and the final file: both stay as
-- they are, and the name is frozen.
UPDATE jobs SET staging_name = display_name, name_source = 'legacy';
