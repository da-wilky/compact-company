-- The customer's timezone when issued, which issue_date and due_date are in; NULL on drafts.
ALTER TABLE invoice ADD COLUMN timezone TEXT;

UPDATE invoice SET timezone = (SELECT c.timezone FROM customer c WHERE c.id = invoice.customer_id) WHERE number IS NOT NULL;

-- Triggers, not a CHECK: ADD COLUMN tests a CHECK against existing rows before the backfill can run.
CREATE TRIGGER invoice_timezone_insert BEFORE INSERT ON invoice
WHEN (NEW.timezone IS NULL) != (NEW.number IS NULL)
BEGIN SELECT RAISE(ABORT, 'invoice timezone must be set exactly when issued'); END;

CREATE TRIGGER invoice_timezone_update BEFORE UPDATE OF number, timezone ON invoice
WHEN (NEW.timezone IS NULL) != (NEW.number IS NULL)
BEGIN SELECT RAISE(ABORT, 'invoice timezone must be set exactly when issued'); END;
