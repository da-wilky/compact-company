CREATE TABLE bank_account (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    iban TEXT UNIQUE,
    ledger_account_id INTEGER NOT NULL UNIQUE REFERENCES ledger_account (id),
    archived INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE bank_import (
    id INTEGER PRIMARY KEY,
    bank_account_id INTEGER NOT NULL REFERENCES bank_account (id),
    -- content hash of the statement file
    file TEXT NOT NULL,
    file_name TEXT,
    format TEXT NOT NULL CHECK (format IN ('camt053', 'csv')),
    imported INTEGER NOT NULL,
    skipped INTEGER NOT NULL,
    user_id INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE bank_transaction (
    id INTEGER PRIMARY KEY,
    bank_account_id INTEGER NOT NULL REFERENCES bank_account (id),
    import_id INTEGER NOT NULL REFERENCES bank_import (id),
    -- ISO dates
    booking_date TEXT NOT NULL,
    value_date TEXT,
    -- EUR cents, money in positive
    amount INTEGER NOT NULL,
    counterparty TEXT,
    counterparty_iban TEXT,
    purpose TEXT,
    reference TEXT,
    -- the same movement in overlapping statements has the same fingerprint
    fingerprint TEXT NOT NULL,
    UNIQUE (bank_account_id, fingerprint)
) STRICT;

CREATE INDEX bank_transaction_date ON bank_transaction (bank_account_id, booking_date);

CREATE TRIGGER bank_transaction_fixed BEFORE UPDATE ON bank_transaction
BEGIN SELECT RAISE(ABORT, 'an imported bank transaction never changes'); END;

-- what a bank transaction pays; undone by reversing everything it booked
CREATE TABLE bank_match (
    id INTEGER PRIMARY KEY,
    bank_transaction_id INTEGER NOT NULL REFERENCES bank_transaction (id),
    user_id INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL,
    undone_at INTEGER,
    undone_by INTEGER REFERENCES user (id)
) STRICT;

CREATE UNIQUE INDEX bank_match_active ON bank_match (bank_transaction_id) WHERE undone_at IS NULL;

ALTER TABLE payment ADD COLUMN bank_match_id INTEGER REFERENCES bank_match (id);
ALTER TABLE receipt_payment ADD COLUMN bank_match_id INTEGER REFERENCES bank_match (id);
ALTER TABLE posting ADD COLUMN bank_match_id INTEGER REFERENCES bank_match (id);
CREATE INDEX posting_bank_match ON posting (bank_match_id);
