-- B2.8–B2.11 (ADR 0015, ADR 0017): queues, priority and manual order; destination rules and the
-- collision policy; the ask-on-collision confirmation. One forward-only migration shared by the
-- batch so the phases never compete for schema versions.

-- Queues. 'main' always exists and cannot be deleted; its displayed name is localized by clients.
CREATE TABLE queues (
    queue_id      TEXT    PRIMARY KEY NOT NULL,
    name          TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    position      INTEGER NOT NULL CHECK (position >= 0),
    max_active    INTEGER NOT NULL CHECK (max_active BETWEEN 1 AND 16),
    -- 0 after Stop queue: its jobs hold with the queue_stopped blocker until Start queue.
    running       INTEGER NOT NULL CHECK (running IN (0, 1)),
    created_at_ms INTEGER NOT NULL
) STRICT;

INSERT INTO queues (queue_id, name, position, max_active, running, created_at_ms)
VALUES ('main', 'Main queue', 0, 3, 1, 0);

-- Every existing job joins the default queue, at normal priority, in its acceptance order.
-- (A REFERENCES clause cannot be added with a non-NULL default; the service keeps queue_id valid.)
ALTER TABLE jobs ADD COLUMN queue_id TEXT NOT NULL DEFAULT 'main';
ALTER TABLE jobs ADD COLUMN priority INTEGER NOT NULL DEFAULT 0 CHECK (priority IN (-1, 0, 1));
ALTER TABLE jobs ADD COLUMN queue_position INTEGER NOT NULL DEFAULT 0;
UPDATE jobs SET queue_position = rowid;

-- The destination category a job was filed under (NULL for jobs accepted before B2.8). The job's
-- destination folder itself was always stored and is never changed by later rule edits.
ALTER TABLE jobs ADD COLUMN category TEXT CHECK (category IS NULL OR category IN
    ('compressed', 'documents', 'music', 'programs', 'video', 'other'));

-- 0 while an ask-on-collision job waits for the user to confirm its free final name.
ALTER TABLE jobs ADD COLUMN name_confirmed INTEGER NOT NULL DEFAULT 1 CHECK (name_confirmed IN (0, 1));

-- Category → absolute folder. A category without a row uses the default download folder.
CREATE TABLE destination_rules (
    category  TEXT PRIMARY KEY NOT NULL CHECK (category IN
        ('compressed', 'documents', 'music', 'programs', 'video', 'other')),
    directory TEXT NOT NULL CHECK (length(directory) BETWEEN 1 AND 1024)
) STRICT;

-- Small service settings (currently only 'on_collision': 'rename' | 'ask').
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
