CREATE TABLE project (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    name TEXT NOT NULL,
    -- in the minor unit of the customer's currency
    hourly_rate INTEGER CHECK (hourly_rate >= 0),
    billable INTEGER NOT NULL DEFAULT 1 CHECK (billable IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX project_customer ON project (customer_id);

CREATE TRIGGER project_no_delete BEFORE DELETE ON project
BEGIN SELECT RAISE(ABORT, 'projects are archived, never deleted'); END;

-- its rate is in the customer's currency, so a project stays with its customer
CREATE TRIGGER project_customer_fixed BEFORE UPDATE OF customer_id ON project
WHEN NEW.customer_id IS NOT OLD.customer_id
BEGIN SELECT RAISE(ABORT, 'a project never changes its customer'); END;

CREATE TABLE activity (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TRIGGER activity_no_delete BEFORE DELETE ON activity
BEGIN SELECT RAISE(ABORT, 'activities are archived, never deleted'); END;

-- an activity has at most one rate per currency; work for a customer in another currency
-- falls back to the project's or the customer's rate
CREATE TABLE activity_rate (
    activity_id INTEGER NOT NULL REFERENCES activity (id),
    currency TEXT NOT NULL,
    hourly_rate INTEGER NOT NULL CHECK (hourly_rate >= 0),
    PRIMARY KEY (activity_id, currency)
) STRICT;

CREATE VIEW project_view AS
SELECT p.id, p.customer_id, c.name AS customer_name, c.number AS customer_number, c.currency,
    (p.archived OR c.archived) AS closed, p.name, p.hourly_rate, p.billable, p.archived,
    p.created_at
FROM project p JOIN customer c ON c.id = p.customer_id;
