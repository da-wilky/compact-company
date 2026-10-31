-- The accepted quote an invoice or project was created from.
ALTER TABLE invoice ADD COLUMN quote_id INTEGER REFERENCES quote (id);
CREATE INDEX invoice_quote ON invoice (quote_id);
ALTER TABLE project ADD COLUMN quote_id INTEGER REFERENCES quote (id);
