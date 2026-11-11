CREATE TABLE layout (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    -- the data schema the layout is written for (ADR 0019)
    schema INTEGER NOT NULL,
    -- JSON array of the document kinds it lays out
    kinds TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- images and fonts uploaded with a layout, in the file store
CREATE TABLE layout_file (
    layout_id INTEGER NOT NULL REFERENCES layout (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    font INTEGER NOT NULL CHECK (font IN (0, 1)),
    hash TEXT NOT NULL,
    PRIMARY KEY (layout_id, name)
) STRICT;

-- Typst packages the owner approved by name and version; the archive is in the file store
CREATE TABLE package (
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    hash TEXT NOT NULL,
    approved_by INTEGER NOT NULL REFERENCES user (id),
    approved_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, name, version)
) STRICT;

-- the approved packages a layout imports, directly or through other packages
CREATE TABLE layout_package (
    layout_id INTEGER NOT NULL REFERENCES layout (id) ON DELETE CASCADE,
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    PRIMARY KEY (layout_id, namespace, name, version),
    FOREIGN KEY (namespace, name, version) REFERENCES package (namespace, name, version)
) STRICT;

-- the layout of a document kind, for every customer or, with customer_id, for one
CREATE TABLE layout_choice (
    kind TEXT NOT NULL,
    customer_id INTEGER REFERENCES customer (id),
    layout_id INTEGER NOT NULL REFERENCES layout (id) ON DELETE CASCADE
) STRICT;

CREATE UNIQUE INDEX layout_choice_default ON layout_choice (kind) WHERE customer_id IS NULL;
CREATE UNIQUE INDEX layout_choice_customer ON layout_choice (kind, customer_id)
WHERE customer_id IS NOT NULL;

-- name of the uploaded layout the document was issued with; none for the built-in one
ALTER TABLE document ADD COLUMN layout TEXT;
