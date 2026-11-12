-- no-transaction
-- Zeilen 32 and 38 change the CHECK on ledger_account, which SQLite only allows by rebuilding the
-- table; foreign keys and the renaming checks are off while it is replaced
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
        '12', '15', '16', '17', '18', '27', '29', '30', '32', '33', '36', '38', '39', '44', '55',
        '56', '57', '58', '60', '63'
    ))
) STRICT;

INSERT INTO ledger_account_new (id, number, name, type, role, archived, euer_line)
SELECT id, number, name, type, role, archived, euer_line FROM ledger_account;

DROP TABLE ledger_account;
ALTER TABLE ledger_account_new RENAME TO ledger_account;

-- the accounts depreciation books on, seeded since; an existing account of the number takes the role
UPDATE ledger_account SET role = 'depreciation' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '6220' ELSE '4830' END;
UPDATE ledger_account SET role = 'low_value_write_off' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '6260' ELSE '4855' END;

INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '6200' ELSE '4822' END,
    'Abschreibungen auf immaterielle Vermögensgegenstände', 'expense', '32'
FROM books;
UPDATE ledger_account SET role = 'depreciation_intangible' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '6200' ELSE '4822' END;

INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '6895' ELSE '2310' END,
    'Anlagenabgänge Sachanlagen (Restbuchwert bei Buchverlust)', 'expense', '38'
FROM books;
UPDATE ledger_account SET role = 'asset_disposal' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '6895' ELSE '2310' END;

CREATE TABLE asset (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    -- the group of the Anlage AVEÜR it is listed in
    kind TEXT NOT NULL CHECK (kind IN ('intangible', 'vehicle', 'office', 'other')),
    -- the fixed-asset account it is carried on
    account_id INTEGER NOT NULL REFERENCES ledger_account (id),
    receipt_line_id INTEGER UNIQUE REFERENCES receipt_line (id),
    -- ISO date
    acquired TEXT NOT NULL,
    -- EUR cents it cost, VAT included where it can't be deducted
    cost INTEGER NOT NULL CHECK (cost > 0),
    -- EUR cents without VAT, which the low-value limit is tested on
    net INTEGER NOT NULL CHECK (net > 0),
    method TEXT NOT NULL CHECK (method IN ('straight_line', 'low_value')),
    -- whole years, for straight-line depreciation
    life_years INTEGER CHECK (life_years BETWEEN 1 AND 50),
    -- ISO date; written off on that day
    disposed TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((method = 'straight_line') = (life_years IS NOT NULL))
) STRICT;

ALTER TABLE posting ADD COLUMN asset_id INTEGER REFERENCES asset (id);
CREATE INDEX posting_asset ON posting (asset_id);

COMMIT;
PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
