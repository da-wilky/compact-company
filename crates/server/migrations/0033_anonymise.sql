-- holds a row only inside the transaction that anonymises a year: then, and only then, the
-- texts of fixed records may be replaced (ADR 0025)
CREATE TABLE anonymising (
    year INTEGER PRIMARY KEY
) STRICT;

DROP TRIGGER posting_fixed;
CREATE TRIGGER posting_fixed BEFORE UPDATE ON posting
WHEN NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'a posting never changes'); END;

DROP TRIGGER document_issued_fixed;
CREATE TRIGGER document_issued_fixed BEFORE UPDATE ON document
WHEN OLD.status = 'issued' AND NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

DROP TRIGGER document_line_issued_update;
CREATE TRIGGER document_line_issued_update BEFORE UPDATE ON document_line
WHEN (SELECT status FROM document WHERE id = OLD.document_id) = 'issued'
    AND NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

DROP TRIGGER receipt_booked_fixed;
CREATE TRIGGER receipt_booked_fixed BEFORE UPDATE ON receipt
WHEN OLD.status = 'booked' AND NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

DROP TRIGGER receipt_line_booked_update;
CREATE TRIGGER receipt_line_booked_update BEFORE UPDATE ON receipt_line
WHEN (SELECT status FROM receipt WHERE id = OLD.receipt_id) = 'booked'
    AND NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'a booked receipt never changes'); END;

DROP TRIGGER bank_transaction_fixed;
CREATE TRIGGER bank_transaction_fixed BEFORE UPDATE ON bank_transaction
WHEN NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'an imported bank transaction never changes'); END;

DROP TRIGGER audit_no_update;
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit
WHEN NOT EXISTS (SELECT 1 FROM anonymising)
BEGIN SELECT RAISE(ABORT, 'audit entries cannot be changed'); END;

-- names and contact data replaced once nothing kept refers to them any more
ALTER TABLE customer ADD COLUMN anonymised INTEGER NOT NULL DEFAULT 0 CHECK (anonymised IN (0, 1));
ALTER TABLE supplier ADD COLUMN anonymised INTEGER NOT NULL DEFAULT 0 CHECK (anonymised IN (0, 1));
ALTER TABLE user ADD COLUMN anonymised INTEGER NOT NULL DEFAULT 0 CHECK (anonymised IN (0, 1));
