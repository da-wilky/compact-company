-- a customer has several billing addresses; documents pick one, the default when they name none
CREATE TABLE customer_address (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customer (id),
    label TEXT,
    street TEXT,
    postcode TEXT,
    city TEXT,
    country TEXT NOT NULL DEFAULT 'DE',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    -- an address an issued document names is archived instead of deleted
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    CHECK (NOT (is_default AND archived))
) STRICT;

CREATE INDEX customer_address_customer ON customer_address (customer_id);
CREATE UNIQUE INDEX customer_address_default ON customer_address (customer_id) WHERE is_default;

INSERT INTO customer_address (customer_id, street, postcode, city, country, is_default)
SELECT id, street, postcode, city, country, 1 FROM customer;

ALTER TABLE customer DROP COLUMN street;
ALTER TABLE customer DROP COLUMN postcode;
ALTER TABLE customer DROP COLUMN city;
ALTER TABLE customer DROP COLUMN country;

ALTER TABLE document ADD COLUMN address_id INTEGER REFERENCES customer_address (id);
