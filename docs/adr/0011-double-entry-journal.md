# Every booking goes through one double-entry journal

Issued invoices, payments, receipts, bank transactions, depreciation and corrections all write balanced postings to one append-only journal on SKR03 or SKR04 accounts, chosen at setup; VAT return, EÜR, open items and the DATEV export are all derived from it. Single-entry would suffice for EÜR, but a UG or GmbH legally needs double entry and retrofitting it means migrating every booking. A posting is never edited or deleted: it is reversed by a new one (GoBD).
