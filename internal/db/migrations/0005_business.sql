CREATE TABLE business (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL,
    company TEXT NOT NULL,
    address TEXT NOT NULL,
    email TEXT NOT NULL,
    web TEXT NOT NULL,
    phone TEXT NOT NULL,
    tax_number TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    bank_iban TEXT NOT NULL,
    bank_bic TEXT NOT NULL,
    tax_mode TEXT NOT NULL CHECK (tax_mode IN ('small_business', 'standard')),
    vat_rate INTEGER NOT NULL CHECK (vat_rate BETWEEN 0 AND 100)
) STRICT;

INSERT INTO business VALUES (1, '', '', '', '', '', '', '', '', '', '', 'small_business', 0);
