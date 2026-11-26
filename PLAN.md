# compact-company – Plan

A self-hosted business suite for one German business: todos and timers, quotes and invoices with e-invoicing and free Typst layouts, receipts, bank import and double-entry books. Open source (AGPL-3.0-or-later). Open work is in the GitHub issues.

## Why

Time tracking, invoicing and bookkeeping usually live in two or three tools with hand-offs between them. Haben covers the books well but has no todos or timers and limits invoice layouts; time trackers stop at the invoice. compact-company does both for one business, with every document laid out by a Typst layout the business controls.

## Decisions

| Area | Decision | ADR |
| --- | --- | --- |
| Scope | Suite for one business; UG/GmbH year-end closing by the tax advisor | 0008 |
| Audience | Built for one business, public repo others may self-host, no support promises | – |
| Tenancy | One business per instance | 0003 |
| Language | Rust, rebuilt from scratch, no data migration from the Go version | 0009 |
| Frontend | Svelte SPA over the app's own JSON API | 0010 |
| Books | Double-entry journal, SKR03 or SKR04 chosen at setup | 0011 |
| Layouts | Uploaded in the app, raw data + Typst helper library, approved packages only | 0012 |
| Tax office | Figures computed, entered by hand in Mein ELSTER | 0013 |
| Login | Passkeys, OIDC optional | 0014 |
| Roles | Owner, staff, accountant | 0015 |
| Storage | SQLite only; files content-addressed on disk or S3 | 0016 |
| Settings | Owner edits business data and defaults; config may pin any key | 0017 |
| Numbers | One range per document type, owner-set format, assigned when issued | 0018 |
| Layout data | Schema-versioned; layouts declare the version they were written for | 0019 |
| EÜR | Payments count their share of what they pay; accounts name their EÜR line | 0021 |
| Closing | A closed year takes no postings; its archive carries checksums | 0022 |
| Depreciation | Computed from the asset, booked per finished year; disposals rebook their year | 0023 |
| Partial invoices | Booked as advances, VAT due when paid; the final invoice deducts paid ones, cancels unpaid | 0024 |
| Retention | Owner deletes files after their period with a confirmation; archives pruned, years anonymised after ten | 0025 |
| Mirror and backup | One-way WebDAV mirror to read; age-encrypted S3 snapshots with rotation to restore | 0026 |
| Process documentation | Generated where the instance knows, questionnaire for the rest, versioned | 0027 |
| Simple books | A view over the journal: accounts by name under their EÜR line, gross amounts, journal hidden; default for a small business | 0028 |
| Languages | UI and documents in German and English | – |
| Platforms | x86_64 and arm64; NixOS module and OCI image | – |

## Scope

**Work**
- Customers with permanent numbers, projects, archiving
- Hourly rates: customer default, project override, activity types with their own rate
- Todos per project: list and board over one shared status list, priority, due date, assignee, comments
- Timers on a project or todo, several running per user; manual entries; week view
- Billable flag and a billable duration next to the tracked one

**Documents** (all rendered by Typst layouts)
- Quote, order confirmation, invoice, cancellation invoice, dunning letter, delivery note, timesheet, acceptance report
- Invoices from unbilled time: grouped by project, todo, activity or not at all; rounding per entry (0/5/10/15/30 min); timesheet as attachment
- Outgoing e-invoices (ZUGFeRD and XRechnung, EN 16931) from the first release, embedded in PDF/A-3
- Mandatory details of § 14 Abs. 4 UStG checked before issuing; legal details of a UG/GmbH (§ 35a GmbHG) on every document
- Sending by email (SMTP) with a sent log per document
- Recurring invoices; partial and final invoices

**Books**
- Journal on SKR03/SKR04; every issued invoice, payment, receipt, bank transaction and depreciation posts to it; corrections only by reversal
- Taxation: Kleinunternehmer (§ 19 UStG), regular with cash basis (Ist), regular with accrual basis (Soll)
- Receipts: upload of PDFs and photos, incoming ZUGFeRD/XRechnung read automatically, others entered by hand; account and VAT per line
- Bank: CAMT.053 and CSV import, matching suggestions against open invoices and receipts, partial payments
- Open items and dunning
- Fixed assets and depreciation (straight-line, low-value assets)
- VAT return and EÜR figures traceable to postings; DATEV export; deadlines as ICS feed and email reminders; year-end archive with checksums
- Audit log of every change that affects the books

**Retention and backup**
- Deletion after the statutory retention periods; Nextcloud mirror over WebDAV; backup to S3
- Process documentation (GoBD Verfahrensdokumentation), generated where the instance knows the answer

**Access and integration**
- Passkeys, optional OIDC; roles owner, staff, accountant
- REST API with scoped tokens (OpenAPI); tokens never issue, send or book

## Non-goals

- Several businesses per instance
- EU and third-country cases: reverse charge (§ 13b UStG), intra-community supplies, ZM
- Balance sheet, P&L and E-Bilanz for UG/GmbH (the tax advisor closes the year from the DATEV export)
- ELSTER transmission (ERiC), payroll, cash book, flat-rate allowances
- PSD2 aggregators; PostgreSQL
- Todo subtasks, sprints, dependencies, Gantt, budgets

## Stack

| Concern | Choice |
| --- | --- |
| HTTP | `axum` |
| DB | SQLite via `sqlx`, compile-time checked queries (offline data for Nix builds) |
| Money | `rust_decimal`, never floats |
| API | `utoipa` OpenAPI → generated TypeScript types for the client |
| Auth | `webauthn-rs` for passkeys, `openidconnect` for OIDC |
| Documents | `typst` + `typst-pdf` in-process, PDF/A-3; EN 16931 CII written with `quick-xml`; KoSIT validator in CI |
| Storage | `object_store` (local disk or S3) |
| Frontend | Svelte 5, SvelteKit with the static adapter, embedded in the binary |
| Nix | `crane` for Rust, `buildNpmPackage` for the client, `dockerTools` for the image, NixOS module with VM test |
