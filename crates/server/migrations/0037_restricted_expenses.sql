-- no-transaction
-- Zeilen 62, 64 and 66 widen the CHECK on ledger_account, which SQLite only allows by
-- rebuilding the table; foreign keys and the renaming checks are off while it is replaced
PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;
BEGIN;

CREATE TABLE ledger_account_new (
    id INTEGER PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('asset', 'liability', 'equity', 'income', 'expense')),
    -- what the posting rules book on it, such as `receivables` or `revenue:19`
    role TEXT UNIQUE,
    archived INTEGER NOT NULL DEFAULT 0,
    -- the Zeile of the Anlage EÜR an account counts on; none for balance sheet accounts
    euer_line TEXT CHECK (euer_line IN (
        '12', '15', '16', '17', '18', '27', '29', '30', '32', '33', '36', '38', '39', '40', '41',
        '42', '43', '44', '45', '46', '47', '48', '49', '50', '51', '52', '53', '54', '55', '56',
        '57', '58', '60', '62', '62_nd', '63', '64', '66', '68', '69', '70', '71', '72', '73', '74'
    ))
) STRICT;

INSERT INTO ledger_account_new (id, number, name, type, role, archived, euer_line)
SELECT id, number, name, type, role, archived, euer_line FROM ledger_account;

DROP TABLE ledger_account;
ALTER TABLE ledger_account_new RENAME TO ledger_account;

-- gifts and meal allowances, seeded since
INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '6610' ELSE '4630' END,
    'Geschenke abzugsfähig ohne § 37b EStG', 'expense', '62'
FROM books;
INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '6620' ELSE '4635' END,
    'Geschenke nicht abzugsfähig ohne § 37b EStG', 'expense', '62_nd'
FROM books;
INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '6674' ELSE '4674' END,
    'Reisekosten Unternehmer Verpflegungsmehraufwand', 'expense', '64'
FROM books;

COMMIT;
PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
