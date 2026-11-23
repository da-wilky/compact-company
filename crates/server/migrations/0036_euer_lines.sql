-- no-transaction
-- Zeilen 40–54 and 68–74 widen the CHECK on ledger_account, which SQLite only allows by
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
        '57', '58', '60', '63', '68', '69', '70', '71', '72', '73', '74'
    ))
) STRICT;

INSERT INTO ledger_account_new (id, number, name, type, role, archived, euer_line)
SELECT id, number, name, type, role, archived, euer_line FROM ledger_account;

DROP TABLE ledger_account;
ALTER TABLE ledger_account_new RENAME TO ledger_account;

-- seeded accounts still on Zeile 60 move to their own line; ones re-mapped by hand stay
UPDATE ledger_account SET euer_line = CASE (SELECT chart FROM books)
    WHEN 'skr04' THEN CASE number
        WHEN '6805' THEN '43' WHEN '6810' THEN '43'
        WHEN '6821' THEN '45'
        WHEN '6825' THEN '46' WHEN '6830' THEN '46'
        WHEN '6400' THEN '49' WHEN '6420' THEN '49'
        WHEN '6800' THEN '51' WHEN '6815' THEN '51' WHEN '6820' THEN '51'
        WHEN '6600' THEN '54'
        WHEN '6530' THEN '70' WHEN '6673' THEN '70'
        ELSE euer_line END
    ELSE CASE number
        WHEN '4920' THEN '43' WHEN '4925' THEN '43'
        WHEN '4945' THEN '45'
        WHEN '4950' THEN '46' WHEN '4955' THEN '46'
        WHEN '4360' THEN '49' WHEN '4380' THEN '49'
        WHEN '4910' THEN '51' WHEN '4930' THEN '51' WHEN '4940' THEN '51'
        WHEN '4600' THEN '54'
        WHEN '4530' THEN '70' WHEN '4673' THEN '70'
        ELSE euer_line END
    END
WHERE euer_line = '60' AND type = 'expense';

COMMIT;
PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
