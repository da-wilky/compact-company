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

**User**:
A person who logs in and tracks time; all users work for the same business.
_Avoid_: Member, account
