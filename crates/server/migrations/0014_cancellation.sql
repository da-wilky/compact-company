-- the invoice line a cancellation invoice's line reverses
ALTER TABLE document_line ADD COLUMN cancels_line_id INTEGER REFERENCES document_line (id);

CREATE INDEX document_line_cancels ON document_line (cancels_line_id);

DROP TRIGGER time_entry_billed_update;
DROP TRIGGER time_entry_billed_delete;

-- an entry on a cancelled line is unbilled again and may go on another invoice, so it may be
-- linked more than once; only one uncancelled line may hold it, which the app checks
CREATE TABLE line_time_entry_new (
    line_id INTEGER NOT NULL REFERENCES document_line (id) ON DELETE CASCADE,
    time_entry_id INTEGER NOT NULL REFERENCES time_entry (id),
    PRIMARY KEY (line_id, time_entry_id)
) STRICT;
INSERT INTO line_time_entry_new SELECT * FROM line_time_entry;
DROP TABLE line_time_entry;
ALTER TABLE line_time_entry_new RENAME TO line_time_entry;
CREATE INDEX line_time_entry_entry ON line_time_entry (time_entry_id);

-- a line counts as billed unless an issued cancellation invoice reverses it
CREATE VIEW billed_line AS
SELECT l.* FROM document_line l
WHERE NOT EXISTS (SELECT 1 FROM document_line c JOIN document d ON d.id = c.document_id
    WHERE c.cancels_line_id = l.id AND d.status = 'issued');

CREATE TRIGGER time_entry_billed_update BEFORE UPDATE ON time_entry
WHEN EXISTS (SELECT 1 FROM line_time_entry x JOIN document_line l ON l.id = x.line_id
    JOIN document d ON d.id = l.document_id WHERE x.time_entry_id = OLD.id AND d.status = 'issued')
BEGIN SELECT RAISE(ABORT, 'billed time never changes'); END;

CREATE TRIGGER time_entry_billed_delete BEFORE DELETE ON time_entry
WHEN EXISTS (SELECT 1 FROM line_time_entry WHERE time_entry_id = OLD.id)
BEGIN SELECT RAISE(ABORT, 'time on an invoice is not deleted'); END;
