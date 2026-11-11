-- the taxation replaces the small-business flag; without one the business owes VAT on accrual
INSERT INTO setting (key, value)
SELECT 'taxation', '"small_business"' FROM setting WHERE key = 'small_business' AND value = 'true';
DELETE FROM setting WHERE key = 'small_business';

-- the books start once; the chart of accounts never changes afterwards
CREATE TABLE books (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    chart TEXT NOT NULL CHECK (chart IN ('skr03', 'skr04')),
    -- ISO date of the first day booked
    start_date TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    started_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

CREATE TABLE ledger_account (
    id INTEGER PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('asset', 'liability', 'equity', 'income', 'expense')),
    -- what the posting rules book on it, such as `receivables` or `revenue:19`
    role TEXT UNIQUE,
    archived INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE posting (
    id INTEGER PRIMARY KEY,
    -- ISO date
    date TEXT NOT NULL,
    description TEXT NOT NULL,
    reverses_id INTEGER UNIQUE REFERENCES posting (id),
    document_id INTEGER REFERENCES document (id),
    payment_id INTEGER REFERENCES payment (id),
    user_id INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX posting_date ON posting (date, id);
CREATE INDEX posting_document ON posting (document_id);
CREATE INDEX posting_payment ON posting (payment_id);

CREATE TABLE posting_line (
    id INTEGER PRIMARY KEY,
    posting_id INTEGER NOT NULL REFERENCES posting (id),
    account_id INTEGER NOT NULL REFERENCES ledger_account (id),
    -- EUR cents, debit positive and credit negative; a posting's lines sum to zero
    amount INTEGER NOT NULL CHECK (amount <> 0)
) STRICT;

CREATE INDEX posting_line_posting ON posting_line (posting_id);
CREATE INDEX posting_line_account ON posting_line (account_id);

CREATE TRIGGER posting_fixed BEFORE UPDATE ON posting
BEGIN SELECT RAISE(ABORT, 'a posting never changes'); END;

CREATE TRIGGER posting_kept BEFORE DELETE ON posting
BEGIN SELECT RAISE(ABORT, 'a posting is never deleted'); END;

CREATE TRIGGER posting_line_fixed BEFORE UPDATE ON posting_line
BEGIN SELECT RAISE(ABORT, 'a posting never changes'); END;

CREATE TRIGGER posting_line_kept BEFORE DELETE ON posting_line
BEGIN SELECT RAISE(ABORT, 'a posting is never deleted'); END;
