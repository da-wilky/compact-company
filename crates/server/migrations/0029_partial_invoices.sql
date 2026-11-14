-- a partial invoice bills an advance; the final invoice bills the whole work less the paid ones
ALTER TABLE document ADD COLUMN invoice_type TEXT CHECK (invoice_type IN ('partial', 'final'));

-- the partial invoices a final invoice took in when issued: deducted if paid, else cancelled
CREATE TABLE final_partial (
    final_id INTEGER NOT NULL REFERENCES document (id),
    partial_id INTEGER NOT NULL UNIQUE REFERENCES document (id),
    state TEXT NOT NULL CHECK (state IN ('deducted', 'cancelled')),
    PRIMARY KEY (final_id, partial_id)
) STRICT;

-- advances received on partial invoices, seeded since; they count as income when paid (Zeile 15)
INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '3272' ELSE '1718' END,
    'Erhaltene, versteuerte Anzahlungen 19 % USt (Verbindlichkeiten)', 'liability', '15'
FROM books;
UPDATE ledger_account SET role = 'advances:19' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '3272' ELSE '1718' END;

INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '3260' ELSE '1711' END,
    'Erhaltene, versteuerte Anzahlungen 7 % USt (Verbindlichkeiten)', 'liability', '15'
FROM books;
UPDATE ledger_account SET role = 'advances:7' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '3260' ELSE '1711' END;

INSERT OR IGNORE INTO ledger_account (number, name, type, euer_line)
SELECT CASE chart WHEN 'skr04' THEN '3280' ELSE '1719' END,
    'Erhaltene Anzahlungen (Verbindlichkeiten)', 'liability', '15'
FROM books;
UPDATE ledger_account SET role = 'advances:0' WHERE role IS NULL AND number =
    CASE (SELECT chart FROM books) WHEN 'skr04' THEN '3280' ELSE '1719' END;

UPDATE ledger_account SET euer_line = '15' WHERE role LIKE 'advances:%' AND euer_line IS NULL;
