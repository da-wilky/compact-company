# Files are deleted after their retention period; a year is anonymised after ten

Amends ADR 0022.

Every stored document has a retention class, counted from the end of the year it belongs to (§ 147 Abs. 4 AO), and may go from 1 January after its period ends:

| Class | Records | Kept | Source |
| --- | --- | --- | --- |
| Booking document | invoices, cancellation, partial and final invoices with their timesheets, receipts with their original e-invoice XML, bank statements | 8 years | § 147 Abs. 1 Nr. 4, Abs. 3 AO; § 14b Abs. 1 UStG |
| Business letter | quotes, order confirmations, acceptance reports, dunning letters | 6 years | § 147 Abs. 1 Nr. 2–3, Abs. 3 AO |
| Delivery note | a delivery note whose invoice is issued | until then | § 147 Abs. 3 Sätze 3–4 AO |
| Books | journal, year-end archives, asset register, process documentation | 10 years | § 147 Abs. 1 Nr. 1, Abs. 3 AO |

A period doesn't end while the tax assessment is still open (§ 147 Abs. 3 Satz 5 AO), which only the owner knows, so nothing is deleted on a schedule: the owner deletes a closed year's files after confirming that no audit, objection or other open assessment concerns it. Held back regardless of the date: the receipt of a fixed asset still in the register or disposed of fewer than 8 years ago, a partial invoice whose final invoice is kept, and a document a kept document of a later year refers to. The record stays with its number, date, amounts and postings; only the file goes, from the store unless another kept record has the same hash, and from the mirror. Backups lose it through rotation.

The year archive is rewritten without the deleted files. It keeps the original `SHA256SUMS`, lists what went in `DELETED` and checksums what it still holds in `SHA256SUMS.pruned`, so the remaining files still pass `sha256sum -c --ignore-missing` against the original sums. The closed year keeps the original archive's hash, and each pruning records the hash of the archive that replaces it.

Ten years after a closed year, oldest first and with the same confirmation, the owner anonymises it unless an asset of it is still in the register or an invoice or receipt of it is open. Its archive, remaining files, superseded process documentation, time entries, comments and sent-mail logs are deleted. Postings keep amounts, accounts and numbers, since later balances rest on them, but lose their texts; so do documents, receipts, bank transactions and audit entries of the year. Customers, suppliers and former users with no record after the year lose their names. The database runs with `secure_delete`, so the old values don't linger in free pages.

Considered: deleting on a schedule, which can't know whether an assessment is open; deleting the records too, which would break later balances and the numbering; and keeping one unpruned archive beside the pruned one, which keeps exactly what was meant to go.
