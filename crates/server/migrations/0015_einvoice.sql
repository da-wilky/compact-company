ALTER TABLE customer ADD COLUMN vat_id TEXT;
-- the buyer reference public customers require on XRechnung invoices
ALTER TABLE customer ADD COLUMN leitweg_id TEXT;
-- whether the customer gets invoices as XRechnung as well as ZUGFeRD
ALTER TABLE customer ADD COLUMN xrechnung INTEGER NOT NULL DEFAULT 0 CHECK (xrechnung IN (0, 1));

-- SHA-256 of the XRechnung XML as issued, in the file store
ALTER TABLE document ADD COLUMN xml TEXT;
