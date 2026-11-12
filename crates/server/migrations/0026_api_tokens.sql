-- a token acts as its user within its scopes; it never issues, sends or books
CREATE TABLE api_token (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id),
    name TEXT NOT NULL,
    token_hash BLOB NOT NULL UNIQUE,
    -- space-separated, such as `work:read work:write`
    scopes TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER
) STRICT;

CREATE INDEX api_token_user ON api_token (user_id);
