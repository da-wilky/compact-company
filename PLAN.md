# compact-company – Plan

A self-hosted business suite for one German business: todos and timers, quotes and invoices with e-invoicing and free Typst layouts, receipts, bank import and double-entry books. Rebuilt in Rust; the Go version stays in use until this one replaces it. Open source (AGPL-3.0-or-later).

## Why

Time tracking, invoicing and bookkeeping usually live in two or three tools with hand-offs between them. Haben covers the books well but has no todos or timers and limits invoice layouts; time trackers stop at the invoice. compact-company does both for one business, with every document laid out by a Typst layout the business controls.

## Decisions

| Area | Decision | ADR |
| --- | --- | --- |
| Scope | Suite for one business; UG/GmbH year-end closing by the tax advisor | 0008 |
| Audience | Built for swilk, public repo others may self-host, no support promises | – |
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

**Books**
- Journal on SKR03/SKR04; every issued invoice, payment, receipt, bank transaction and depreciation posts to it; corrections only by reversal
- Taxation: Kleinunternehmer (§ 19 UStG), regular with cash basis (Ist), regular with accrual basis (Soll)
- Receipts: upload of PDFs and photos, incoming ZUGFeRD/XRechnung read automatically, others entered by hand; account and VAT per line
- Bank: CAMT.053 and CSV import, matching suggestions against open invoices and receipts, partial payments
- Open items and dunning
- Fixed assets and depreciation (straight-line, low-value assets)
- VAT return and EÜR figures traceable to postings; DATEV export; deadlines as ICS feed and email reminders; year-end archive with checksums
- Audit log of every change that affects the books

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

Crate names are candidates; verify each before the foundation phase settles them.

## Phases

### Phase 1 – Foundation
- Cargo workspace, Svelte client, flake (dev shell, package, image, checks), CI
- Config (TOML, secrets only from files), SQLite migrations, file storage on disk/S3
- Passkeys, optional OIDC, sessions, CSRF, roles; audit log
- API skeleton with OpenAPI and generated client types; i18n catalog shared by server and client
- NixOS module and OCI image, x86_64 and arm64
- Done when: an owner registers a passkey on a fresh instance and invites a staff user

### Phase 2 – Work
- Customers, projects, activities, rates, todos with list and board, statuses
- Parallel timers, manual entries, week view, billable duration
- Dark mode
- Rate limiting of unauthenticated endpoints (sign-in, setup, invitation and OIDC starts)
- Done when: a staff user tracks time on a todo of a new customer's project

### Phase 3 – Documents (first release, replaces the Go version)
- Layout engine: built-in layouts, upload with fonts, package approval, sample render and § 14 check, per-customer override
- Quotes → order confirmation → invoice; invoices from time; cancellation invoices; delivery note, timesheet, acceptance report
- ZUGFeRD and XRechnung, validated by KoSIT in CI
- Email sending with sent log; overview (running timers, unbilled time, open invoices)
- Done when: swilk's invoices and quotes are issued from it with the corporate-design layout, and the Go version is retired

### Phase 4 – Books
- Journal, SKR03/SKR04, posting rules for invoices and payments under each taxation
- Receipts with incoming e-invoice parsing
- Bank import and matching, open items, dunning letters
- Done when: a quarter of swilk's receipts and bank statements is booked and every open invoice is matched

### Phase 5 – Tax and exports
- VAT return and EÜR figures with drill-down to postings
- DATEV export, deadlines (ICS, reminders), year-end archive
- Accountant role in use; API tokens with scopes
- Done when: a VAT return and an EÜR are entered in Mein ELSTER from its figures, and the DATEV export imports into a DATEV-compatible tool without errors

### Phase 6 – Fixed assets
- Assets, straight-line depreciation, low-value assets; postings and EÜR lines

### Later
- FinTS bank fetch (free; needs a product registration with the Deutsche Kreditwirtschaft)
- AI extraction of receipts (optional endpoint), IMAP receipt inbox, MCP server
- Recurring invoices, partial and final invoices

## Open questions

1. Retention: how long receipts and issued documents are kept, and whether deletion after the period is supported.
2. Process documentation (GoBD Verfahrensdokumentation): generated from settings, or written by hand?

## Context (swilk)

- Brand and Typst library for swilk's layouts: `~/workspace/swilk/gewerbe/corporate-design` (`typst/lib/vorlagen.typ`, `typst/firma.toml`, private `typst/firma.local.toml`); never commit them, the repo is public
- Server: pi5dd (arm64), `/etc/nix-config/hosts/pi5dd/apps/`
- Login today: Keycloak `https://user.swilk.eu/realms/public`
- Tax: Kleinunternehmer (§ 19 UStG); receiving e-invoices is mandatory, issuing is not
