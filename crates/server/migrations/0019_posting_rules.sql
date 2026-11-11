-- a payment recorded by mistake is reversed, not deleted
ALTER TABLE payment ADD COLUMN reversed_at INTEGER;
ALTER TABLE payment ADD COLUMN reversed_by INTEGER REFERENCES user (id);
-- the EUR cents that arrived for an invoice in another currency (ADR 0020)
ALTER TABLE payment ADD COLUMN eur_amount INTEGER CHECK (eur_amount > 0);
-- the ledger account the money arrived on; none while the books are not started
ALTER TABLE payment ADD COLUMN account_id INTEGER REFERENCES ledger_account (id);
