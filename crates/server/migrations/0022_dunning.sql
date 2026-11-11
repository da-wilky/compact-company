-- no-transaction
-- a new document kind changes the CHECK on document, which SQLite only allows by rebuilding the
-- table; foreign keys and the renaming checks are off while document is replaced
PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;
BEGIN;

CREATE TABLE document_new (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('quote', 'order_confirmation', 'invoice', 'cancellation',
        'delivery_note', 'acceptance_report', 'dunning')),
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
    pdf TEXT,
    layout TEXT,
    valid_until TEXT,
    pricing TEXT CHECK (pricing IN ('fixed', 'effort')),
    source_id INTEGER REFERENCES document (id),
    due_date TEXT,
    service_from TEXT,
    service_to TEXT,
    timesheet INTEGER NOT NULL DEFAULT 0 CHECK (timesheet IN (0, 1)),
    rounding INTEGER NOT NULL DEFAULT 0 CHECK (rounding IN (0, 5, 10, 15, 30)),
    xml TEXT,
    -- a dunning letter's level: 1 reminder, 2 dunning letter, 3 final dunning letter
    dunning_level INTEGER CHECK (dunning_level BETWEEN 1 AND 3),
    CHECK ((status = 'draft') = (number IS NULL)),
    CHECK ((status = 'draft') = (data IS NULL)),
    CHECK ((kind = 'dunning') = (dunning_level IS NOT NULL)),
    UNIQUE (kind, number)
) STRICT;

INSERT INTO document_new (id, kind, status, customer_id, project_id, language, currency, title,
    intro, outro, number, issue_date, data, issued_at, issued_by, created_at, updated_at, pdf,
    layout, valid_until, pricing, source_id, due_date, service_from, service_to, timesheet,
    rounding, xml)
SELECT id, kind, status, customer_id, project_id, language, currency, title, intro, outro, number,
    issue_date, data, issued_at, issued_by, created_at, updated_at, pdf, layout, valid_until,
    pricing, source_id, due_date, service_from, service_to, timesheet, rounding, xml
FROM document;

DROP TABLE document;
ALTER TABLE document_new RENAME TO document;

CREATE INDEX document_customer ON document (customer_id);
CREATE INDEX document_kind_status ON document (kind, status);
CREATE INDEX document_source ON document (source_id);

CREATE TRIGGER document_issued_fixed BEFORE UPDATE ON document
WHEN OLD.status = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document never changes'); END;

CREATE TRIGGER document_issued_kept BEFORE DELETE ON document
WHEN OLD.status = 'issued'
BEGIN SELECT RAISE(ABORT, 'an issued document is never deleted'); END;

COMMIT;
PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
