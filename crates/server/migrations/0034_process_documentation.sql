-- the owner's answers to the questions of the process documentation the instance can't know
CREATE TABLE process_answer (
    key TEXT PRIMARY KEY,
    answer TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_by INTEGER NOT NULL REFERENCES user (id)
) STRICT;

-- each state of the process documentation, valid until the next one's date (ADR 0027)
CREATE TABLE process_version (
    id INTEGER PRIMARY KEY,
    -- ISO date
    valid_from TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    -- content hashes of the facts and answers as JSON and of the PDF in each language
    data TEXT NOT NULL,
    pdf_de TEXT NOT NULL,
    pdf_en TEXT NOT NULL,
    -- JSON array of the fact and answer keys that differ from the version before
    changes TEXT NOT NULL
) STRICT;

CREATE TRIGGER process_version_fixed BEFORE UPDATE ON process_version
BEGIN SELECT RAISE(ABORT, 'a version of the process documentation never changes'); END;

DROP VIEW stored_file;
CREATE VIEW stored_file AS
SELECT subject, hash FROM (
    SELECT 'document:' || id AS subject, pdf AS hash FROM document WHERE pdf IS NOT NULL
    UNION ALL SELECT 'document:' || id, xml FROM document WHERE xml IS NOT NULL
    UNION ALL SELECT 'receipt:' || id, file FROM receipt WHERE file IS NOT NULL
    UNION ALL SELECT 'bank_import:' || id, file FROM bank_import
    UNION ALL SELECT 'year:' || year, archive FROM year_archive WHERE archive IS NOT NULL
    UNION ALL SELECT 'layout:' || layout_id, hash FROM layout_file
    UNION ALL SELECT 'package:' || namespace || '/' || name || ':' || version, hash FROM package
    UNION ALL SELECT 'process:' || id, data FROM process_version
    UNION ALL SELECT 'process:' || id, pdf_de FROM process_version
    UNION ALL SELECT 'process:' || id, pdf_en FROM process_version
) s
WHERE NOT EXISTS (SELECT 1 FROM deleted_file d WHERE d.subject = s.subject AND d.hash = s.hash);
