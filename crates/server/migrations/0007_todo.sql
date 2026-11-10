-- One ordered list for the whole business; each status counts as done or not.
CREATE TABLE todo_status (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1))
) STRICT;

INSERT INTO todo_status (id, name, position, done) VALUES
    (1, 'Offen', 1, 0),
    (2, 'In Arbeit', 2, 0),
    (3, 'Wartet', 3, 0),
    (4, 'Erledigt', 4, 1);

CREATE TABLE todo (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES project (id),
    title TEXT NOT NULL,
    description TEXT,
    status_id INTEGER NOT NULL REFERENCES todo_status (id),
    priority TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high')),
    -- a calendar day without a time zone
    due_date TEXT CHECK (due_date IS NULL OR date(due_date) IS due_date),
    assignee_id INTEGER REFERENCES user (id),
    -- order within its status on the board
    position INTEGER NOT NULL,
    created_by INTEGER NOT NULL REFERENCES user (id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX todo_project ON todo (project_id);
CREATE INDEX todo_status_position ON todo (status_id, position);

CREATE TABLE todo_comment (
    id INTEGER PRIMARY KEY,
    todo_id INTEGER NOT NULL REFERENCES todo (id),
    user_id INTEGER NOT NULL REFERENCES user (id),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX todo_comment_todo ON todo_comment (todo_id);

CREATE VIEW todo_view AS
SELECT t.id, t.project_id, p.name AS project_name, p.customer_id, c.name AS customer_name,
    t.title, t.description, t.status_id, s.done, t.priority, t.due_date, t.assignee_id,
    u.name AS assignee_name, t.position,
    (SELECT count(*) FROM todo_comment m WHERE m.todo_id = t.id) AS comments,
    t.created_by, t.created_at, t.updated_at
FROM todo t
JOIN todo_status s ON s.id = t.status_id
JOIN project p ON p.id = t.project_id
JOIN customer c ON c.id = p.customer_id
LEFT JOIN user u ON u.id = t.assignee_id;
