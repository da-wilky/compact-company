-- files deleted after their retention period, with the owner's confirmation (ADR 0025)
CREATE TABLE retention_deletion (
    id INTEGER PRIMARY KEY,
    year INTEGER NOT NULL REFERENCES closed_year (year),
    at INTEGER NOT NULL,
    user_id INTEGER NOT NULL REFERENCES user (id),
    -- what the owner confirmed: no audit, objection or other open assessment concerns the year
    confirmation TEXT NOT NULL
) STRICT;

-- a record's file that is gone; the record itself stays with its number, date and postings
CREATE TABLE deleted_file (
    -- such as `document:12`, `receipt:3` or `bank_import:2`
    subject TEXT NOT NULL,
    hash TEXT NOT NULL,
    deletion_id INTEGER NOT NULL REFERENCES retention_deletion (id),
    PRIMARY KEY (subject, hash)
) STRICT;

-- a year archive without the deleted files, replacing the one before it
CREATE TABLE pruned_archive (
    id INTEGER PRIMARY KEY,
    year INTEGER NOT NULL REFERENCES closed_year (year),
    archive TEXT NOT NULL,
    size INTEGER NOT NULL,
    deletion_id INTEGER NOT NULL UNIQUE REFERENCES retention_deletion (id)
) STRICT;

-- a year anonymised ten years after it was closed: its archive and files are gone, its postings
-- stay without the names in them
CREATE TABLE anonymised_year (
    year INTEGER PRIMARY KEY REFERENCES closed_year (year),
    at INTEGER NOT NULL,
    user_id INTEGER NOT NULL REFERENCES user (id),
    confirmation TEXT NOT NULL
) STRICT;

-- the archive a closed year has now: the latest pruned one, none once anonymised
CREATE VIEW year_archive AS
SELECT c.year,
    CASE WHEN n.year IS NULL THEN coalesce(p.archive, c.archive) END AS archive,
    CASE WHEN n.year IS NULL THEN coalesce(p.size, c.size) END AS size,
    p.id IS NOT NULL AS pruned,
    n.year IS NOT NULL AS anonymised
FROM closed_year c
LEFT JOIN pruned_archive p ON p.id = (SELECT max(id) FROM pruned_archive WHERE year = c.year)
LEFT JOIN anonymised_year n ON n.year = c.year;

-- every file a record still needs, by the record
CREATE VIEW stored_file AS
SELECT subject, hash FROM (
    SELECT 'document:' || id AS subject, pdf AS hash FROM document WHERE pdf IS NOT NULL
    UNION ALL SELECT 'document:' || id, xml FROM document WHERE xml IS NOT NULL
    UNION ALL SELECT 'receipt:' || id, file FROM receipt WHERE file IS NOT NULL
    UNION ALL SELECT 'bank_import:' || id, file FROM bank_import
    UNION ALL SELECT 'year:' || year, archive FROM year_archive WHERE archive IS NOT NULL
    UNION ALL SELECT 'layout:' || layout_id, hash FROM layout_file
    UNION ALL SELECT 'package:' || namespace || '/' || name || ':' || version, hash FROM package
) s
WHERE NOT EXISTS (SELECT 1 FROM deleted_file d WHERE d.subject = s.subject AND d.hash = s.hash);
