-- money received for an issued invoice, recorded by hand until bank statements are imported
CREATE TABLE payment (
    id INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id),
    -- ISO date the money arrived
    date TEXT NOT NULL,
    -- in the minor unit of the invoice's currency
    amount INTEGER NOT NULL CHECK (amount > 0),
    note TEXT,
    user_id INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX payment_document ON payment (document_id);
