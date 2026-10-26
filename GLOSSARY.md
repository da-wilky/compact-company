# compact-company

Customers, time tracking and invoices for small businesses.

## Language

**Customer**:
A person or organization that work is done for and invoices go to.
_Avoid_: Client, account

**Customer number**:
The permanent, never-reused identifier of a customer (e.g. `K-001`), assigned when the customer is created.

**Project**:
A body of work for one customer that time is tracked against.
_Avoid_: Job, task

**Hourly rate**:
The price of one hour of work: the customer's default rate, unless the project overrides it.

**Time entry**:
A span of work with a start, an end and a description.
_Avoid_: Time record, timesheet row, booking

**Timer**:
A time entry that has started but not yet ended.

**Unassigned entry**:
A time entry not yet attached to a project.

**Billable**:
Whether a time entry is meant to be charged to the customer; inherited from the project's default.

**Unbilled entry**:
A billable time entry not yet on an issued invoice.

**Archived**:
A customer or project hidden from everyday use and closed to new time, but kept with all its history.

**Claimed entry**:
A time entry on a draft or issued invoice; no other invoice can bill it.

**User**:
A person who logs in and tracks time; all users work for the same business.
_Avoid_: Member, account

**Default**:
A business-wide setting that new users and new customers start with; a user's language and timezone follow it until the user chooses their own.
_Avoid_: System default, preset

**Business**:
The business an instance serves; it sends every invoice.
_Avoid_: Seller, issuer, company

**Invoice**:
A bill to one customer, made of lines.
_Avoid_: Bill

**Service period**:
When the billed work was done: from the start of the invoice's first time entry to the end of its last. An invoice without time entries may have one set by hand; without one it was supplied on its issue date.
_Avoid_: Billing period

**Draft**:
An invoice still being prepared: editable, unnumbered, and deletable.

**Issue**:
To give a draft its invoice number and freeze it for good.

**Issued invoice**:
An invoice that has been issued; it can never be edited or deleted.

**Cancellation invoice**:
An issued invoice that reverses some or all lines of an earlier issued invoice.
_Avoid_: Storno, credit note

**Paid**:
An issued invoice whose payment has been received.

**Amount due**:
What the customer still owes on an issued invoice: its total less whatever its cancellation invoices reverse.

**Open invoice**:
An issued invoice, not a cancellation invoice, that is neither paid nor fully cancelled.
_Avoid_: Outstanding, unpaid

**Overdue**:
An open invoice whose due date has passed.
