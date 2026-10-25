CREATE TABLE time_entry (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id),
    project_id INTEGER REFERENCES project (id),
    start_time INTEGER NOT NULL,
    end_time INTEGER,
    description TEXT NOT NULL DEFAULT '',
    billable INTEGER,
    -- Reserved for Phase 2 invoices; always NULL in Phase 1.
    invoice_id INTEGER,
    CHECK (end_time IS NULL OR end_time >= start_time),
    CHECK ((project_id IS NULL) = (billable IS NULL))
) STRICT;

CREATE INDEX time_entry_user_start ON time_entry (user_id, start_time);
CREATE INDEX time_entry_project ON time_entry (project_id);
