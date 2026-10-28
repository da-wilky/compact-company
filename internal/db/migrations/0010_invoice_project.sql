-- 1: the invoice bills every project of its customer, including later ones; 0: only those in invoice_project.
ALTER TABLE invoice ADD COLUMN all_projects INTEGER NOT NULL DEFAULT 1 CHECK (all_projects IN (0, 1));

CREATE TABLE invoice_project (
    invoice_id INTEGER NOT NULL REFERENCES invoice (id) ON DELETE CASCADE,
    -- A project can only be deleted while it has no entries, so it never billed anything.
    project_id INTEGER NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    PRIMARY KEY (invoice_id, project_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX invoice_project_project ON invoice_project (project_id);
