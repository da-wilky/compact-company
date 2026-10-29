CREATE TABLE todo_status (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL UNIQUE,
    done INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO todo_status (id, name, position, done) VALUES
    (1, 'Open', 1, 0),
    (2, 'In progress', 2, 0),
    (3, 'Waiting', 3, 0),
    (4, 'Done', 4, 1);

ALTER TABLE defaults ADD COLUMN working_status_id INTEGER REFERENCES todo_status (id);
UPDATE defaults SET working_status_id = 2;

CREATE TABLE todo (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES project (id),
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status_id INTEGER NOT NULL REFERENCES todo_status (id),
    assignee_id INTEGER REFERENCES user (id),
    -- 0 low, 1 normal, 2 high
    priority INTEGER NOT NULL DEFAULT 1 CHECK (priority IN (0, 1, 2)),
    -- ISO date, a calendar day with no timezone
    due_date TEXT CHECK (due_date IS NULL OR date(due_date) IS due_date),
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX todo_by_project ON todo (project_id);
CREATE INDEX todo_by_status ON todo (status_id);

ALTER TABLE time_entry ADD COLUMN todo_id INTEGER REFERENCES todo (id) ON DELETE SET NULL;

CREATE INDEX time_entry_todo ON time_entry (todo_id);

-- An entry's project is always its todo's project: setting a todo of another project is a bug,
-- changing the project clears the todo. A todo moves before its entries do.
CREATE TRIGGER time_entry_todo_insert BEFORE INSERT ON time_entry
WHEN NEW.todo_id IS NOT NULL AND NEW.project_id IS NOT (SELECT project_id FROM todo WHERE id = NEW.todo_id)
BEGIN SELECT RAISE(ABORT, 'time entry project must be its todo''s project'); END;

CREATE TRIGGER time_entry_todo_update BEFORE UPDATE OF todo_id ON time_entry
WHEN NEW.todo_id IS NOT NULL AND NEW.project_id IS NOT (SELECT project_id FROM todo WHERE id = NEW.todo_id)
BEGIN SELECT RAISE(ABORT, 'time entry project must be its todo''s project'); END;

CREATE TRIGGER time_entry_project_clears_todo AFTER UPDATE OF project_id ON time_entry
WHEN NEW.todo_id IS NOT NULL AND NEW.project_id IS NOT (SELECT project_id FROM todo WHERE id = NEW.todo_id)
BEGIN UPDATE time_entry SET todo_id = NULL WHERE id = NEW.id; END;
