-- Dates are ISO YYYY-MM-DD in the customer's timezone.
CREATE TABLE quote (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    project_id INTEGER REFERENCES project (id),
    subject TEXT NOT NULL CHECK (subject != ''),
    language TEXT NOT NULL,
    currency TEXT NOT NULL,
    pricing TEXT NOT NULL CHECK (pricing IN ('fixed', 'effort')),
    valid_until TEXT NOT NULL,
    number TEXT UNIQUE,
    send_date TEXT,
    accepted_date TEXT,
    declined_date TEXT,
    snapshot TEXT,
    pdf_path TEXT,
    pdf_sha256 TEXT,
    CHECK ((number IS NULL) = (send_date IS NULL)),
    CHECK (accepted_date IS NULL OR declined_date IS NULL),
    CHECK ((accepted_date IS NULL AND declined_date IS NULL) OR number IS NOT NULL)
) STRICT;

CREATE INDEX quote_customer ON quote (customer_id);
CREATE INDEX quote_project ON quote (project_id);

CREATE TABLE quote_line (
    id INTEGER PRIMARY KEY,
    quote_id INTEGER NOT NULL REFERENCES quote (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    detail TEXT NOT NULL,
    -- thousandths
    quantity INTEGER NOT NULL,
    unit TEXT NOT NULL,
    -- minor units of the quote's currency
    unit_price INTEGER NOT NULL,
    interval TEXT NOT NULL CHECK (interval IN ('once', 'monthly', 'yearly'))
) STRICT;

CREATE INDEX quote_line_quote ON quote_line (quote_id, position);

CREATE TABLE quote_section (
    id INTEGER PRIMARY KEY,
    quote_id INTEGER NOT NULL REFERENCES quote (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    heading TEXT NOT NULL,
    -- Typst markup, stored as typed
    body TEXT NOT NULL
) STRICT;

CREATE INDEX quote_section_quote ON quote_section (quote_id, position);
