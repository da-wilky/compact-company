-- every attempt to email an issued document, kept as written
CREATE TABLE document_send (
    id INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id),
    at INTEGER NOT NULL,
    user_id INTEGER NOT NULL REFERENCES user (id),
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    -- what the mail server answered when it refused; none once sent
    error TEXT
) STRICT;

CREATE INDEX document_send_document ON document_send (document_id);
