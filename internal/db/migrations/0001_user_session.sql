CREATE TABLE user (
    id INTEGER PRIMARY KEY,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    display_name TEXT NOT NULL,
    email TEXT NOT NULL,
    language TEXT NOT NULL,
    timezone TEXT NOT NULL,
    parallel_entries INTEGER NOT NULL,
    UNIQUE (issuer, subject)
) STRICT;

CREATE TABLE session (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id) ON DELETE CASCADE,
    last_seen INTEGER NOT NULL
) STRICT;

CREATE INDEX session_user_id ON session (user_id);
