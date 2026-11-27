-- where documents to the address are emailed and the e-invoice is delivered; the customer's
-- email when none
ALTER TABLE customer_address ADD COLUMN email TEXT;
