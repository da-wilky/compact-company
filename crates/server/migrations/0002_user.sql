CREATE TABLE user (
    id INTEGER PRIMARY KEY,
    handle BLOB NOT NULL UNIQUE,
    name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'staff', 'accountant')),
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE passkey (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id),
    credential_id BLOB NOT NULL UNIQUE,
    name TEXT NOT NULL,
    passkey TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
) STRICT;

CREATE INDEX passkey_user ON passkey (user_id);

CREATE TABLE session (
    token_hash BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id),
    csrf_token TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
) STRICT;

CREATE INDEX session_user ON session (user_id);

-- A started WebAuthn or OIDC exchange, consumed once by its finishing request.
CREATE TABLE ceremony (
    id_hash BLOB PRIMARY KEY,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
