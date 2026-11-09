# SQLite only, documents on disk or S3

One business per instance with a handful of users fits SQLite, which keeps backup to one database snapshot and the SQL to one dialect; supporting PostgreSQL as well would double migrations and tests, and gapless numbering locks differently there. PDFs, receipts, fonts and uploads are stored content-addressed by SHA-256, never overwritten, on disk by default or in an S3-compatible bucket, so ten years of receipts don't grow the database.
