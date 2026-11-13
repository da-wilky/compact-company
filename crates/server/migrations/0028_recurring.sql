-- an invoice drafted again and again on its dates; it only ever creates drafts
CREATE TABLE recurring_invoice (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    project_id INTEGER REFERENCES project (id),
    title TEXT,
    intro TEXT,
    outro TEXT,
    interval TEXT NOT NULL CHECK (interval IN ('monthly', 'quarterly', 'yearly')),
    -- the service period is the interval starting on a date (advance) or the one just ended
    period TEXT NOT NULL CHECK (period IN ('advance', 'arrears')),
    -- ISO date the dates count from; the next is `occurrences` intervals after it
    starts_on TEXT NOT NULL,
    occurrences INTEGER NOT NULL DEFAULT 0 CHECK (occurrences >= 0),
    -- ISO date of the last date drafted, inclusive
    ends_on TEXT,
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    -- whose name the drafts are created in
    created_by INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE recurring_line (
    id INTEGER PRIMARY KEY,
    recurring_id INTEGER NOT NULL REFERENCES recurring_invoice (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    -- decimal as text
    quantity TEXT NOT NULL,
    unit TEXT NOT NULL CHECK (unit IN ('hour', 'day', 'month', 'piece', 'lump_sum')),
    unit_price INTEGER,
    vat_rate TEXT,
    project_id INTEGER REFERENCES project (id),
    activity_id INTEGER REFERENCES activity (id),
    UNIQUE (recurring_id, position)
) STRICT;

-- the recurring invoice a draft came from, and the date it was drafted for, once only
ALTER TABLE document ADD COLUMN recurring_id INTEGER REFERENCES recurring_invoice (id);
ALTER TABLE document ADD COLUMN recurring_date TEXT;
CREATE UNIQUE INDEX document_recurring ON document (recurring_id, recurring_date);
