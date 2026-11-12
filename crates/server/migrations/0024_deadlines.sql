-- the secret link a calendar app subscribes to; renewing it replaces the old one
CREATE TABLE calendar_feed (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    token_hash BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    created_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

-- each deadline is reminded of once a week before and once the day before
CREATE TABLE reminder (
    deadline TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('week', 'day')),
    sent_at INTEGER NOT NULL,
    PRIMARY KEY (deadline, kind)
) STRICT;
