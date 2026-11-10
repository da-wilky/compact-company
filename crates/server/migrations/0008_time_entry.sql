CREATE TABLE time_entry (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES user (id),
    project_id INTEGER NOT NULL REFERENCES project (id),
    todo_id INTEGER REFERENCES todo (id),
    activity_id INTEGER REFERENCES activity (id),
    description TEXT,
    started_at INTEGER NOT NULL,
    -- a running timer has no end yet
    ended_at INTEGER,
    billable INTEGER NOT NULL CHECK (billable IN (0, 1)),
    -- seconds charged when that is not the whole tracked duration
    billable_secs INTEGER CHECK (billable_secs >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
) STRICT;

CREATE INDEX time_entry_user_start ON time_entry (user_id, started_at);
CREATE INDEX time_entry_project ON time_entry (project_id);
CREATE INDEX time_entry_todo ON time_entry (todo_id);
CREATE INDEX time_entry_running ON time_entry (user_id) WHERE ended_at IS NULL;

-- time on a todo always belongs to the todo's project
CREATE TRIGGER time_entry_todo_insert BEFORE INSERT ON time_entry
WHEN NEW.todo_id IS NOT NULL
    AND NEW.project_id IS NOT (SELECT project_id FROM todo WHERE id = NEW.todo_id)
BEGIN SELECT RAISE(ABORT, 'a time entry belongs to its todo''s project'); END;

CREATE TRIGGER time_entry_todo_update BEFORE UPDATE OF project_id, todo_id ON time_entry
WHEN NEW.todo_id IS NOT NULL
    AND NEW.project_id IS NOT (SELECT project_id FROM todo WHERE id = NEW.todo_id)
BEGIN SELECT RAISE(ABORT, 'a time entry belongs to its todo''s project'); END;

CREATE TRIGGER todo_project_with_time BEFORE UPDATE OF project_id ON todo
WHEN NEW.project_id IS NOT OLD.project_id
    AND EXISTS (SELECT 1 FROM time_entry WHERE todo_id = NEW.id)
BEGIN SELECT RAISE(ABORT, 'a todo with time stays on its project'); END;

CREATE VIEW time_entry_view AS
SELECT e.id, e.user_id, u.name AS user_name, e.project_id, p.name AS project_name,
    p.customer_id, c.name AS customer_name, e.todo_id, t.title AS todo_title, e.activity_id,
    a.name AS activity_name, e.description, e.started_at, e.ended_at, e.billable,
    e.billable_secs, e.created_at, e.updated_at
FROM time_entry e
JOIN user u ON u.id = e.user_id
JOIN project p ON p.id = e.project_id
JOIN customer c ON c.id = p.customer_id
LEFT JOIN todo t ON t.id = e.todo_id
LEFT JOIN activity a ON a.id = e.activity_id;
