-- Counters for permanent numbers; a number once handed out is never handed out again.
CREATE TABLE sequence (
    name TEXT PRIMARY KEY,
    last INTEGER NOT NULL
) STRICT;

INSERT INTO sequence (name, last) VALUES ('customer', 0);

CREATE TABLE customer (
    id INTEGER PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    address TEXT,
    email TEXT,
    language TEXT NOT NULL CHECK (language IN ('de', 'en')),
    currency TEXT NOT NULL,
    -- in the minor unit of `currency`
    hourly_rate INTEGER CHECK (hourly_rate >= 0),
    note TEXT,
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TRIGGER customer_no_delete BEFORE DELETE ON customer
BEGIN SELECT RAISE(ABORT, 'customers are archived, never deleted'); END;

CREATE TRIGGER customer_number_fixed BEFORE UPDATE OF number ON customer
WHEN NEW.number IS NOT OLD.number
BEGIN SELECT RAISE(ABORT, 'customer numbers never change'); END;
