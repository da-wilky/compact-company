# compact-company

A business suite for one German business: work, documents and books.

## Language

### Business and people

**Business** (de: Unternehmen):
The business an instance serves; it issues every document and keeps the books.
_Avoid_: Seller, issuer, company, tenant

**User** (de: Benutzer):
A person who signs in; every user works for the same business and has exactly one role.
_Avoid_: Member, account

**Owner** (de: Inhaber):
The role that may do everything, including settings, issuing and booking.

**Staff** (de: Mitarbeiter):
The role that works on todos and its own time and sees customers and projects.
_Avoid_: Employee, member

**Accountant** (de: Steuerberater):
The role that reads the books and takes exports but changes nothing.
_Avoid_: Tax advisor, bookkeeper

**Invitation** (de: Einladung):
A one-time link from the owner that lets one named person become a user with a given role.

**Passkey** (de: Passkey):
A credential on a user's device that signs them in; a user may have several and never fewer than one.
_Avoid_: Password, key

**Audit log** (de: Änderungsprotokoll):
The append-only record of who changed what and when, with the values before and after.
_Avoid_: History, changelog

**Customer** (de: Kunde):
A person or organization that work is done for and documents go to.
_Avoid_: Client, contact, account

**Customer number** (de: Kundennummer):
The permanent, never-reused identifier of a customer (e.g. `K-001`), assigned when the customer is created.

**Supplier** (de: Lieferant):
A person or organization the business buys from; receipts come from suppliers.
_Avoid_: Vendor, creditor, contact

**Archived** (de: archiviert):
A customer, supplier, project or activity hidden from everyday use and closed to new work, but kept with all its history.

### Work

**Project** (de: Projekt):
A body of work for one customer that todos and time belong to.
_Avoid_: Job, task

**Activity** (de: Tätigkeit):
A kind of work, such as consulting or development, with its own hourly rate; a todo or time entry may name one.
_Avoid_: Service, category, task type

**Hourly rate** (de: Stundensatz):
The price of one hour of work: the activity's rate if the work names one, else the project's, else the customer's default.

**Todo** (de: Aufgabe):
A piece of work within one project, optionally assigned to one user; time tracked on a todo belongs to its project.
_Avoid_: Task, ticket, issue

**Priority** (de: Priorität):
How urgent a todo is: low, normal or high; normal unless chosen otherwise.

**Status** (de: Status):
Where a todo stands, from one ordered list shared by the whole business; a status either counts as done or not.
_Avoid_: Column, stage, state

**Comment** (de: Kommentar):
A note a user adds to a todo; comments are kept as written.
_Avoid_: Note, reply

**Time entry** (de: Zeiteintrag):
A span of work by one user on a project or todo, with a duration and a description.
_Avoid_: Time record, timesheet row, booking

**Timer** (de: Timer):
A time entry that has started but not yet ended; a user may run several at once.

**Billable** (de: abrechenbar):
Whether a time entry is meant to be charged to the customer.

**Billable duration** (de: abrechenbare Dauer):
The part of a time entry's duration that is charged; the full duration unless set otherwise.

**Unbilled entry** (de: nicht abgerechneter Zeiteintrag):
A billable time entry not yet on an issued invoice.

### Documents

**Document** (de: Dokument):
Anything the business issues to a customer, laid out by a layout: quote, order confirmation, invoice, cancellation invoice, dunning letter, delivery note, timesheet or acceptance report.
_Avoid_: Beleg (that is a receipt), file

**Layout** (de: Layout):
The design a document is rendered with; each document type has a default, and a customer may have its own.
_Avoid_: Template, theme, design

**Draft** (de: Entwurf):
A document still being prepared: editable, unnumbered and deletable.

**Issue** (de: ausstellen):
To give a draft its number and freeze it for good; an issued document can never be edited or deleted.
_Avoid_: Finalize, lock, send

**Send** (de: versenden):
To email an issued document to its customer.

**Quote** (de: Angebot):
An offer of work to one customer that stays open until a validity date.
_Avoid_: Offer, estimate, Kostenvoranschlag

**Fixed price** (de: Festpreis):
A quote whose total is what the customer will be invoiced.

**By effort** (de: nach Aufwand):
A quote whose total is an estimate; the customer is invoiced for the time actually spent.
_Avoid_: Estimate, Kostenvoranschlag

**Accepted** (de: angenommen):
An issued quote the customer has agreed to.

**Declined** (de: abgelehnt):
An issued quote the customer has turned down.

**Expired** (de: abgelaufen):
An issued quote neither accepted nor declined whose validity date has passed.

**Order confirmation** (de: Auftragsbestätigung):
A document confirming an accepted quote as an order.

**Invoice** (de: Rechnung):
A bill to one customer, made of lines.
_Avoid_: Bill

**E-invoice** (de: E-Rechnung):
An invoice whose content is also carried in a structured, machine-readable form (ZUGFeRD or XRechnung).

**Service period** (de: Leistungszeitraum):
When the invoiced work was done; from its time entries if it has any, otherwise set by hand or the issue date.
_Avoid_: Billing period

**Cancellation invoice** (de: Stornorechnung):
An issued invoice that reverses some or all lines of an earlier issued invoice.
_Avoid_: Storno, credit note

**Dunning letter** (de: Mahnung):
A document reminding a customer of an overdue invoice, at an increasing level.
_Avoid_: Reminder, payment reminder

**Delivery note** (de: Lieferschein):
A document listing what was delivered, without prices.

**Timesheet** (de: Stundennachweis):
A document listing the time entries behind an invoice.
_Avoid_: Time report

**Acceptance report** (de: Abnahmeprotokoll):
A document in which the customer confirms that delivered work is accepted.

**Snippet** (de: Textbaustein):
A named, reusable piece of text in one language, copied into a document and independent of it afterwards.
_Avoid_: Template, text block

**Overdue** (de: überfällig):
An open item, or a todo not yet done, whose due date has passed.

### Books

**Receipt** (de: Beleg):
A document from a supplier for something the business paid or owes, such as a supplier's invoice or a ticket.
_Avoid_: Expense, bill, voucher

**Journal** (de: Journal):
The complete, append-only record of every posting, in order.
_Avoid_: Ledger, log

**Posting** (de: Buchung):
One balanced entry in the journal: amounts moved between ledger accounts on a date, with the document, receipt or bank transaction behind it.
_Avoid_: Booking, transaction, entry

**Book** (de: buchen):
To record a posting in the journal.

**Reversal** (de: Stornobuchung):
A posting that exactly undoes an earlier one; the only way to correct a posting.
_Avoid_: Correction, deletion

**Ledger account** (de: Sachkonto):
A named, numbered account that postings move amounts between.
_Avoid_: Account (alone), category

**Chart of accounts** (de: Kontenrahmen):
The set of ledger accounts a business books on: SKR03 or SKR04, chosen once.

**Bank transaction** (de: Bankumsatz):
One movement on the business's bank account, as imported from a statement.
_Avoid_: Transaction (alone), payment

**Payment** (de: Zahlung):
Money received for an invoice or paid for a receipt; an invoice or receipt may have several.

**Match** (de: zuordnen):
To link a bank transaction to the invoices or receipts it pays, which records their payments.
_Avoid_: Reconcile, assign

**Open item** (de: offener Posten):
An issued invoice or a receipt whose amount due is not yet zero.
_Avoid_: Outstanding, unpaid

**Amount due** (de: offener Betrag):
What is still owed on an invoice or receipt: its total less its payments and, for an invoice, its cancellation invoices.

**Paid** (de: bezahlt):
An invoice or receipt whose amount due is zero.

**Taxation** (de: Besteuerungsart):
How the business owes VAT: as a small business (no VAT), on a cash basis or on an accrual basis.

**Small business** (de: Kleinunternehmer):
A business exempt from charging VAT under § 19 UStG; its invoices show no VAT.

**Cash basis** (de: Istversteuerung):
Taxation under which VAT on an invoice is owed when the payment is received.

**Accrual basis** (de: Sollversteuerung):
Taxation under which VAT on an invoice is owed for the period it is issued in.

**VAT return** (de: Umsatzsteuer-Voranmeldung):
The monthly or quarterly declaration of VAT owed and input VAT reclaimed.
_Avoid_: UStVA, tax return

**EÜR** (de: Einnahmenüberschussrechnung):
The yearly profit statement of a business that is not required to keep a balance sheet: income less expenses as paid.
_Avoid_: Profit and loss, P&L

**Fixed asset** (de: Anlagegut):
Something the business bought to use for longer than a year, such as a laptop, written off over its useful life.
_Avoid_: Asset (alone), equipment

**Depreciation** (de: Abschreibung):
The part of a fixed asset's cost booked as an expense in one year.
_Avoid_: AfA, write-off

**Low-value asset** (de: geringwertiges Wirtschaftsgut):
A fixed asset cheap enough to be written off in full in the year it is bought.
_Avoid_: GWG
