# Simple books are a view over the journal, not a second ledger

A small business filing an EÜR needs income, expenses, receipts and the Anlage EÜR, not account numbers, a journal or open items. Simple books show only that. Each ledger account is offered by name under the EÜR line it counts on, without its number. A receipt line asks for a gross amount (and a VAT rate if the business charges VAT), not for net, rate and tax. Books, open items and account numbers are hidden from the owner but stay reachable, and the accountant always sees them. The overview shows the year's income, expenses and profit.

Underneath, nothing changes: every receipt, payment and bank match still books balanced postings on SKR03/SKR04 accounts (ADR 0011), and the EÜR is still computed from them (ADR 0021). We rejected a single-entry cash book because it would mean a second source for the EÜR, a second set of GoBD guarantees (reversal, closing, audit), and a migration once the business outgrows it. With a view, switching between simple and full books in either direction is just a setting.

The setting `simple_books` is on by default for a small business and off otherwise, and the owner can change it either way.
