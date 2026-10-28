-- The project a line bills; generated lines take it from their time entries, NULL for lines without one.
ALTER TABLE invoice_line ADD COLUMN project_id INTEGER REFERENCES project (id);
