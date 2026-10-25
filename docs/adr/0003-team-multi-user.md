# Multi-user means one business with several users

One instance serves one business. Customers, projects and invoices are shared; time entries and timers belong to the user who tracked them. Multi-tenancy (several independent businesses per instance) was rejected: it would move seller data, numbering and the Typst template from config into the database with a UI, roughly doubling Phase 1. Roles are out of scope.
