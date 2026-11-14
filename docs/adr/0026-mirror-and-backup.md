# A WebDAV mirror to read, an encrypted S3 backup to restore

Amends ADR 0016.

The file store stays the only place the app reads documents from. Two copies leave it, for different purposes:

A mirror copies every stored document into a readable folder tree over WebDAV, such as Nextcloud, named in German (`2026/Rechnungen/RE-2026-001.pdf`, `2026/Belege/…`, `Verfahrensdokumentation/…`), never by content hash. It is one-way: what changes there is overwritten by the next copy. Each change to a record queues it in the database, in the transaction that changes it; a background task copies what the record should have and deletes what it no longer has, retrying with backoff, so the app works the same while the mirror is down. A full resync copies everything again.

A backup is a daily snapshot of the database (`VACUUM INTO`, consistent while the app writes) and every stored file, in an S3 bucket of its own. Everything is encrypted with age to a public key in the config; the identity to decrypt is only needed by `restore` and stays off the server, so neither a leaked bucket nor a compromised instance reads old backups. Files are uploaded once, under their plaintext content hash. Rotation keeps the newest snapshot of each of the last days and months and deletes files no kept snapshot lists, which is how deletions under ADR 0025 leave the backup. `restore` refuses a non-empty data directory and checks every file and year archive against its checksum afterwards.

Considered: Nextcloud as the primary store, which would make the app depend on it being up; a two-way sync, which would let a changed file in Nextcloud alter a booked record; and server-side bucket encryption, which the instance itself could still read.
