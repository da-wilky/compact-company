# A closed year takes no more postings

The owner closes a finished year, oldest first; a database trigger then refuses every posting dated in it, so a late correction books into the open year as GoBD expects. Closing writes one archive (journal, accounts, DATEV export, tax figures, issued documents, receipts, statements and the audit log up to the closing) with a SHA-256 of every file, stored content-addressed like any other file. Reopening a year is not offered: it would make the archive's checksum meaningless.
