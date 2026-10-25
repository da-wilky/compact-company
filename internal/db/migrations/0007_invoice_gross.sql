-- Minor units of the invoice's currency, negative on cancellations; set when issued.
ALTER TABLE invoice ADD COLUMN gross INTEGER;

-- Snapshot totals are formatted with exactly the currency's decimals, so dropping the point gives minor units.
UPDATE invoice SET gross = CAST(replace(snapshot ->> '$.totals.gross', '.', '') AS INTEGER) WHERE snapshot IS NOT NULL;
