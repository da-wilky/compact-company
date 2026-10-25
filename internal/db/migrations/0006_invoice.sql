-- Dates are ISO YYYY-MM-DD in the customer's timezone.
CREATE TABLE invoice (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('invoice', 'cancellation')),
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    number TEXT UNIQUE,
    cancels_id INTEGER REFERENCES invoice (id),
    range_from TEXT,
    range_to TEXT,
    service_from TEXT NOT NULL,
    service_to TEXT NOT NULL,
    grouping TEXT CHECK (grouping IN ('project', 'entry')),
    rounding_minutes INTEGER NOT NULL,
    payment_days INTEGER NOT NULL,
    issue_date TEXT,
    due_date TEXT,
    paid_date TEXT,
    reason TEXT NOT NULL DEFAULT '',
    snapshot TEXT,
    pdf_path TEXT,
    pdf_sha256 TEXT,
    currency TEXT NOT NULL,
    CHECK ((kind = 'cancellation') = (cancels_id IS NOT NULL)),
    CHECK ((number IS NULL) = (issue_date IS NULL)),
    CHECK (paid_date IS NULL OR number IS NOT NULL)
) STRICT;

CREATE INDEX invoice_customer ON invoice (customer_id);

CREATE TABLE invoice_line (
    id INTEGER PRIMARY KEY,
    invoice_id INTEGER NOT NULL REFERENCES invoice (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    generated INTEGER NOT NULL,
    description TEXT NOT NULL,
    detail TEXT NOT NULL,
    -- thousandths
    quantity INTEGER NOT NULL,
    unit TEXT NOT NULL,
    -- minor units of the invoice's currency
    unit_price INTEGER NOT NULL,
    cancels_line_id INTEGER UNIQUE REFERENCES invoice_line (id)
) STRICT;

CREATE INDEX invoice_line_invoice ON invoice_line (invoice_id, position);

ALTER TABLE time_entry DROP COLUMN invoice_id;
-- An entry is claimed while it references a line; deleting the line releases it.
ALTER TABLE time_entry ADD COLUMN invoice_line_id INTEGER REFERENCES invoice_line (id) ON DELETE SET NULL;
CREATE INDEX time_entry_invoice_line ON time_entry (invoice_line_id);
