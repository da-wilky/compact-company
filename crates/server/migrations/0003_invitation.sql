CREATE TABLE invitation (
    id INTEGER PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'staff', 'accountant')),
    created_by INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    accepted_by INTEGER REFERENCES user (id)
) STRICT;

CREATE TABLE oidc_identity (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id INTEGER NOT NULL UNIQUE REFERENCES user (id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (issuer, subject)
) STRICT;

CREATE VIEW user_view AS
SELECT
    user.id,
    user.name,
    user.role,
    user.active,
    user.created_at,
    (SELECT count(*) FROM passkey WHERE passkey.user_id = user.id) AS passkeys,
    EXISTS (SELECT 1 FROM oidc_identity WHERE oidc_identity.user_id = user.id) AS oidc
FROM user;
