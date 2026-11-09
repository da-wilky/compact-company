-- Who changed what, when, with the values before and after; append-only (GoBD).
CREATE TABLE audit (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    user_id INTEGER NOT NULL REFERENCES user (id),
    action TEXT NOT NULL,
    subject TEXT NOT NULL,
    before TEXT,
    after TEXT
) STRICT;

CREATE INDEX audit_subject ON audit (subject);
CREATE INDEX audit_user ON audit (user_id);

CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit
BEGIN
    SELECT RAISE(ABORT, 'audit entries cannot be changed');
END;

CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit
BEGIN
    SELECT RAISE(ABORT, 'audit entries cannot be deleted');
END;
