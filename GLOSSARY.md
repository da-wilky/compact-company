# compact-company

Customers, time tracking and invoices for small businesses.

## Language

**Customer** (de: Kunde):
A person or organization that work is done for and invoices go to.
_Avoid_: Client, account

**Customer number** (de: Kundennummer):
The permanent, never-reused identifier of a customer (e.g. `K-001`), assigned when the customer is created.

**Project** (de: Projekt):
A body of work for one customer that time is tracked against.
_Avoid_: Job, task

**Hourly rate** (de: Stundensatz):
The price of one hour of work: the customer's default rate, unless the project overrides it.

**Time entry** (de: Zeiteintrag):
A span of work with a start, an end and a description.
_Avoid_: Time record, timesheet row, booking

**Timer** (de: Timer):
A time entry that has started but not yet ended.

**Todo** (de: Aufgabe):
A piece of work within one project, optionally assigned to one user; time tracked on a todo belongs to its project.
_Avoid_: Task, ticket, issue

**Priority** (de: Priorität):
How urgent a todo is: low, normal or high; normal unless chosen otherwise.

**Status** (de: Status):
Where a todo stands, from one ordered list shared by the whole business; a status either counts as done or not. New todos get the first status.
_Avoid_: Column, stage, state

**Working status** (de: Arbeitsstatus):
The status a todo moves to from the first status when someone starts a timer on it; a business may have none.

**Unassigned entry** (de: nicht zugeordneter Zeiteintrag):
A time entry not yet attached to a project.
_Avoid_: No project (that describes invoice lines outside every project section)

**Billable** (de: abrechenbar):
Whether a time entry is meant to be charged to the customer; inherited from the project's default.

**Unbilled entry** (de: nicht abgerechneter Zeiteintrag):
A billable time entry not yet on an issued invoice.

**Archived** (de: archiviert):
A customer or project hidden from everyday use and closed to new time, but kept with all its history.

**Claimed entry**:
A time entry on a draft or issued invoice; no other invoice can bill it.

**User** (de: Benutzer):
A person who logs in and tracks time; all users work for the same business.
_Avoid_: Member, account

**Default** (de: Voreinstellung):
A business-wide setting that new users and new customers start with; a user's language and timezone follow it until the user chooses their own.
_Avoid_: System default, preset

**Business** (de: Unternehmen):
The business an instance serves; it sends every invoice.
_Avoid_: Seller, issuer, company

**Invoice** (de: Rechnung):
A bill to one customer, made of lines; it bills either all of the customer's projects, including ones added later, or a fixed choice of them.
_Avoid_: Bill

**Project section**:
The lines of an invoice that belong to one project, under a heading with the project's name; lines without a project come after all sections.

**Service period** (de: Leistungszeitraum):
When the billed work was done: from the start of the invoice's first time entry to the end of its last. An invoice without time entries may have one set by hand; without one it was supplied on its issue date.
_Avoid_: Billing period

**Draft** (de: Entwurf):
An invoice still being prepared: editable, unnumbered, and deletable.

**Issue** (de: stellen):
To give a draft its invoice number and freeze it for good.

**Issued invoice** (de: gestellte Rechnung):
An invoice that has been issued; it can never be edited or deleted.

**Cancellation invoice** (de: Stornorechnung):
An issued invoice that reverses some or all lines of an earlier issued invoice.
_Avoid_: Storno, credit note

**Paid** (de: bezahlt):
An issued invoice whose payment has been received.

**Amount due** (de: offener Betrag):
What the customer still owes on an issued invoice: its total less whatever its cancellation invoices reverse.

**Open invoice** (de: offene Rechnung):
An issued invoice, not a cancellation invoice, that is neither paid nor fully cancelled.
_Avoid_: Outstanding, unpaid

**Overdue** (de: überfällig):
An open invoice, or a todo not yet done, whose due date has passed.
