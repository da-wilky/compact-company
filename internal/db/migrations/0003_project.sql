CREATE TABLE project (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    name TEXT NOT NULL,
    hourly_rate INTEGER,
    billable INTEGER NOT NULL DEFAULT 1,
    archived INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX project_customer ON project (customer_id);
