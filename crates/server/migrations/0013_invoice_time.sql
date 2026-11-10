-- ISO dates: when an invoice is due, and its service period when set by hand
ALTER TABLE document ADD COLUMN due_date TEXT;
ALTER TABLE document ADD COLUMN service_from TEXT;
ALTER TABLE document ADD COLUMN service_to TEXT;
-- whether the invoice carries a timesheet of its time entries
ALTER TABLE document ADD COLUMN timesheet INTEGER NOT NULL DEFAULT 0 CHECK (timesheet IN (0, 1));

-- the time entries a line charges; an entry is on one line at most
CREATE TABLE line_time_entry (
    line_id INTEGER NOT NULL REFERENCES document_line (id) ON DELETE CASCADE,
    time_entry_id INTEGER NOT NULL UNIQUE REFERENCES time_entry (id),
    PRIMARY KEY (line_id, time_entry_id)
) STRICT;

-- billed time stays as it was billed
CREATE TRIGGER time_entry_billed_update BEFORE UPDATE ON time_entry
WHEN EXISTS (SELECT 1 FROM line_time_entry x JOIN document_line l ON l.id = x.line_id
    JOIN document d ON d.id = l.document_id WHERE x.time_entry_id = OLD.id AND d.status = 'issued')
BEGIN SELECT RAISE(ABORT, 'billed time never changes'); END;

CREATE TRIGGER time_entry_billed_delete BEFORE DELETE ON time_entry
WHEN EXISTS (SELECT 1 FROM line_time_entry WHERE time_entry_id = OLD.id)
BEGIN SELECT RAISE(ABORT, 'time on an invoice is not deleted'); END;
-- the rounding of each time entry in minutes, as last used to add time; also for its timesheet
ALTER TABLE document ADD COLUMN rounding INTEGER NOT NULL DEFAULT 0 CHECK (rounding IN (0, 5, 10, 15, 30));
