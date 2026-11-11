-- documents need a postal address in parts: e-invoices require postcode, city and country
ALTER TABLE customer DROP COLUMN address;
ALTER TABLE customer ADD COLUMN street TEXT;
ALTER TABLE customer ADD COLUMN postcode TEXT;
ALTER TABLE customer ADD COLUMN city TEXT;
ALTER TABLE customer ADD COLUMN country TEXT NOT NULL DEFAULT 'DE';

-- one counter per number range and year; a range without the year in its format uses year 0
CREATE TABLE number_range (
    name TEXT NOT NULL,
    year INTEGER NOT NULL,
    last INTEGER NOT NULL,
    PRIMARY KEY (name, year)
) STRICT;

CREATE TABLE document (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('quote', 'order_confirmation', 'invoice', 'cancellation',
        'delivery_note', 'acceptance_report')),
    status TEXT NOT NULL CHECK (status IN ('draft', 'issued')),
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    project_id INTEGER REFERENCES project (id),
    language TEXT NOT NULL CHECK (language IN ('de', 'en')),
    currency TEXT NOT NULL,
    title TEXT,
    intro TEXT,
    outro TEXT,
    number TEXT,
    -- ISO date in the business's timezone
    issue_date TEXT,
    -- the layout data as issued (ADR 0019); a draft's is computed when read
    data TEXT,
    issued_at INTEGER,
    issued_by INTEGER REFERENCES user (id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((status = 'draft') = (number IS NULL)),
    CHECK ((status = 'draft') = (data IS NULL)),
    UNIQUE (kind, number)
) STRICT;

CREATE INDEX document_customer ON document (customer_id);
CREATE INDEX document_kind_status ON document (kind, status);

CREATE TRIGGER document_issued_fixed BEFORE UPDATE ON document
WHEN OLD.status = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

CREATE TRIGGER document_issued_kept BEFORE DELETE ON document
WHEN OLD.status = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document is never deleted'); END;

CREATE TABLE document_line (
    id INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    -- decimal as text
    quantity TEXT NOT NULL,
    unit TEXT NOT NULL CHECK (unit IN ('hour', 'day', 'month', 'piece', 'lump_sum')),
    -- in the minor unit of the document's currency; without one, a draft line with a project
    -- takes the hourly rate when read, and issuing stores it
    unit_price INTEGER,
    -- percent, decimal as text; a draft line without one takes the business default when read
    vat_rate TEXT,
    project_id INTEGER REFERENCES project (id),
    activity_id INTEGER REFERENCES activity (id),
    UNIQUE (document_id, position)
) STRICT;

CREATE TRIGGER document_line_issued_insert BEFORE INSERT ON document_line
WHEN (SELECT status FROM document WHERE id = NEW.document_id) = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

CREATE TRIGGER document_line_issued_update BEFORE UPDATE ON document_line
WHEN (SELECT status FROM document WHERE id = OLD.document_id) = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

CREATE TRIGGER document_line_issued_delete BEFORE DELETE ON document_line
WHEN (SELECT status FROM document WHERE id = OLD.document_id) = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;
