CREATE TABLE supplier (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    street TEXT,
    postcode TEXT,
    city TEXT,
    country TEXT NOT NULL DEFAULT 'DE',
    vat_id TEXT,
    iban TEXT,
    email TEXT,
    -- where its receipts' lines are booked unless chosen otherwise
    account_id INTEGER REFERENCES ledger_account (id),
    archived INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE receipt (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('draft', 'booked')),
    supplier_id INTEGER REFERENCES supplier (id),
    -- the supplier's own number for it
    number TEXT,
    -- ISO dates
    date TEXT,
    due_date TEXT,
    currency TEXT NOT NULL,
    -- EUR per unit of a receipt in another currency (ADR 0020), decimal as text
    exchange_rate TEXT,
    note TEXT,
    -- content hash of the uploaded file
    file TEXT,
    file_name TEXT,
    file_type TEXT,
    -- the e-invoice format it was read from
    format TEXT CHECK (format IN ('zugferd', 'cii', 'ubl')),
    booked_at INTEGER,
    booked_by INTEGER REFERENCES user (id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (status = 'draft' OR (supplier_id IS NOT NULL AND date IS NOT NULL))
) STRICT;

CREATE INDEX receipt_supplier ON receipt (supplier_id);

CREATE TRIGGER receipt_booked_fixed BEFORE UPDATE ON receipt
WHEN OLD.status = 'booked'
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

CREATE TRIGGER receipt_booked_kept BEFORE DELETE ON receipt
WHEN OLD.status = 'booked'
BEGIN SELECT RAISE(ABORT, 'a booked receipt is never deleted'); END;

CREATE TABLE receipt_line (
    id INTEGER PRIMARY KEY,
    receipt_id INTEGER NOT NULL REFERENCES receipt (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    account_id INTEGER REFERENCES ledger_account (id),
    -- in the minor unit of the receipt's currency
    net INTEGER NOT NULL,
    -- percent, decimal as text
    vat_rate TEXT NOT NULL,
    tax INTEGER NOT NULL,
    UNIQUE (receipt_id, position)
) STRICT;

CREATE TRIGGER receipt_line_booked_insert BEFORE INSERT ON receipt_line
WHEN (SELECT status FROM receipt WHERE id = NEW.receipt_id) = 'booked'
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

CREATE TRIGGER receipt_line_booked_update BEFORE UPDATE ON receipt_line
WHEN (SELECT status FROM receipt WHERE id = OLD.receipt_id) = 'booked'
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

CREATE TRIGGER receipt_line_booked_delete BEFORE DELETE ON receipt_line
WHEN (SELECT status FROM receipt WHERE id = OLD.receipt_id) = 'booked'
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

-- a receipt booked by mistake is reversed; it stays booked with its reversal
CREATE TABLE receipt_reversal (
    receipt_id INTEGER PRIMARY KEY REFERENCES receipt (id),
    reversed_at INTEGER NOT NULL,
    reversed_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

-- money paid for a booked receipt
CREATE TABLE receipt_payment (
    id INTEGER PRIMARY KEY,
    receipt_id INTEGER NOT NULL REFERENCES receipt (id),
    date TEXT NOT NULL,
    -- in the minor unit of the receipt's currency
    amount INTEGER NOT NULL CHECK (amount <> 0),
    -- the EUR cents that left, for a receipt in another currency
    eur_amount INTEGER CHECK (eur_amount <> 0),
    account_id INTEGER NOT NULL REFERENCES ledger_account (id),
    note TEXT,
    user_id INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL,
    reversed_at INTEGER,
    reversed_by INTEGER REFERENCES user (id)
) STRICT;

CREATE INDEX receipt_payment_receipt ON receipt_payment (receipt_id);

ALTER TABLE posting ADD COLUMN receipt_id INTEGER REFERENCES receipt (id);
ALTER TABLE posting ADD COLUMN receipt_payment_id INTEGER REFERENCES receipt_payment (id);
CREATE INDEX posting_receipt ON posting (receipt_id);
CREATE INDEX posting_receipt_payment ON posting (receipt_payment_id);
