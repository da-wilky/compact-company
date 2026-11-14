-- every backup attempt; a run without finished_at and error was cut off
CREATE TABLE backup_run (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    -- the snapshot's name in the bucket
    snapshot TEXT,
    -- files uploaded by this run; the others were in the bucket already
    uploaded INTEGER,
    error TEXT
) STRICT;
