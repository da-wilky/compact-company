-- the language the mirror's names were last copied in; a change copies everything again
CREATE TABLE mirror_language (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    language TEXT NOT NULL CHECK (language IN ('de', 'en'))
) STRICT;
