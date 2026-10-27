CREATE TABLE defaults (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    language TEXT NOT NULL,
    timezone TEXT NOT NULL,
    currency TEXT NOT NULL,
    parallel_entries INTEGER NOT NULL
) STRICT;

INSERT INTO defaults VALUES (1, 'en', 'UTC', 'EUR', 0);
