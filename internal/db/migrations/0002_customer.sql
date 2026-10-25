CREATE TABLE sequence (
    name TEXT NOT NULL,
    year INTEGER,
    last_value INTEGER NOT NULL
) STRICT;

-- UNIQUE (name, year) would not catch duplicates with a NULL year.
CREATE UNIQUE INDEX sequence_name_year ON sequence (name, ifnull(year, -1));

INSERT INTO sequence (name, year, last_value) VALUES ('customer', NULL, 0);

CREATE TABLE customer (
    id INTEGER PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    email TEXT NOT NULL,
    currency TEXT NOT NULL,
    invoice_language TEXT NOT NULL,
    timezone TEXT NOT NULL,
    hourly_rate INTEGER NOT NULL,
    note TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
) STRICT;
