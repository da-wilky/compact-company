-- Reusable quote sections, copied into a quote and independent of it afterwards.
CREATE TABLE snippet (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (name != ''),
    language TEXT NOT NULL,
    heading TEXT NOT NULL,
    -- Typst markup, stored as typed
    body TEXT NOT NULL
) STRICT;
