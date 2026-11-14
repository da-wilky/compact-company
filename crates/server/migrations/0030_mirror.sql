-- records whose files the mirror still has to catch up with, such as `receipt:3`
CREATE TABLE mirror_pending (
    subject TEXT PRIMARY KEY,
    due_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    -- counts queueing, so a copy finishing late keeps a change queued meanwhile
    seq INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    failed_at INTEGER,
    -- copy again even what the mirror should already hold, for a full resync
    force INTEGER NOT NULL DEFAULT 0 CHECK (force IN (0, 1))
) STRICT;

-- what the mirror holds, by the record it was copied for
CREATE TABLE mirror_file (
    path TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    hash TEXT NOT NULL,
    copied_at INTEGER NOT NULL
) STRICT;

CREATE INDEX mirror_file_subject ON mirror_file (subject);
