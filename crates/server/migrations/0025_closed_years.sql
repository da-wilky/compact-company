-- a closed year takes no more postings; its archive is kept in the file store
CREATE TABLE closed_year (
    year INTEGER PRIMARY KEY,
    -- SHA-256 of the archive in the file store
    archive TEXT NOT NULL,
    size INTEGER NOT NULL,
    closed_at INTEGER NOT NULL,
    closed_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

CREATE TRIGGER closed_year_fixed BEFORE UPDATE ON closed_year
BEGIN SELECT RAISE(ABORT, 'a closed year stays closed'); END;

CREATE TRIGGER closed_year_kept BEFORE DELETE ON closed_year
BEGIN SELECT RAISE(ABORT, 'a closed year stays closed'); END;

CREATE TRIGGER posting_in_closed_year BEFORE INSERT ON posting
WHEN NEW.date <= (SELECT max(year) || '-12-31' FROM closed_year)
BEGIN SELECT RAISE(ABORT, 'the year is closed'); END;
