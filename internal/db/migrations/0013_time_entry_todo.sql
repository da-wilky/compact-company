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
