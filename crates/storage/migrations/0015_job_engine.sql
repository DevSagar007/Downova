-- ADR 0043: the engine a download runs on, chosen when it is added and never changed.
--   jobs.engine  'file' (the native Rust engine) or 'media' (the optional yt-dlp engine).
--                Every job stored before this migration is an ordinary file.
ALTER TABLE jobs ADD COLUMN engine TEXT NOT NULL DEFAULT 'file' CHECK (engine IN ('file', 'media'));
