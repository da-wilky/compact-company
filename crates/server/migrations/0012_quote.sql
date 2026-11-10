-- ISO date until which an issued quote stands
ALTER TABLE document ADD COLUMN valid_until TEXT;
-- a quote's total is either what will be invoiced or an estimate of the time it takes
ALTER TABLE document ADD COLUMN pricing TEXT CHECK (pricing IN ('fixed', 'effort'));
-- the document this one follows from: the quote of an order confirmation or invoice, the invoice
-- of a cancellation invoice
ALTER TABLE document ADD COLUMN source_id INTEGER REFERENCES document (id);

CREATE INDEX document_source ON document (source_id);

-- kept apart from the quote, which never changes once issued
CREATE TABLE quote_outcome (
    document_id INTEGER PRIMARY KEY REFERENCES document (id),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'declined')),
    decided_at INTEGER NOT NULL,
    decided_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

CREATE TABLE snippet (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    language TEXT NOT NULL CHECK (language IN ('de', 'en')),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
