-- the billing address its drafts are addressed to; the customer's default when none
ALTER TABLE recurring_invoice ADD COLUMN address_id INTEGER REFERENCES customer_address (id);
