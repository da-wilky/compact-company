# compact-company – Plan

A small, self-hosted tool for one-person businesses: customers, time tracking and invoices rendered with Typst. Minimal, fast, beautiful. Open source.

Starting point for a fresh session: read this file, settle the open decisions, then start with phase 1.

## Why

TimeTracker and similar tools are overloaded (CRM, pipelines, inventory, chat, …) and their invoice PDFs can't match a Typst layout. compact-company does few things well and delegates the invoice layout entirely to a Typst template.

## Goals

- Customers with automatic customer numbers (`K-001`)
- Projects per customer, hourly rates (customer default, project override)
- Time tracking: start/stop timer, manual entries, week view
- Invoices from unbilled time entries plus free line items, rendered by a user-supplied Typst template, stored immutably
- Overview: open amounts, overdue invoices, hours per month
- Single binary, SQLite, trivial backup, NixOS module

## Non-goals (for now)

- Customer portal (maybe later, see "Later")
- Todos / project management, CRM, quotes, expenses
- Multi-user teams, roles
- E-invoicing (ZUGFeRD/XRechnung) – Kleinunternehmer are exempt from issuing; revisit when needed

## Open decisions (settle first)

1. **Language:** Go (recommended: CRUD and forms are its sweet spot, fast iteration) or Rust (if preferred for enjoyment/maintenance; ~20–30 % more effort).
2. **License:** e.g. AGPL-3.0 (keeps hosted forks open) or MIT/Apache-2.0.
3. **UI language:** English default with German translation from the start, or German only first.

## Stack

| | Go | Rust |
| --- | --- | --- |
| HTTP | `net/http` | `axum` |
| Templates | `html/template` | `askama` |
| Interactivity | htmx, no JS build step | same |
| DB | SQLite (`modernc.org/sqlite`, no cgo), plain SQL migrations | SQLite via `sqlx` (offline mode for Nix builds) |
| Auth | OIDC via `coreos/go-oidc` | `openidconnect` + `tower-sessions` |
| PDF | `typst` CLI as subprocess | same |
| Nix | `buildGoModule` | `crane` |

Server-rendered HTML, one CSS file, no framework. Typst must be on `PATH` (the Nix package wraps it in).

## Architecture

- One binary: `compact-company serve --config config.toml`
- Data dir: `app.db` (SQLite, WAL), `invoices/<year>/<number>.pdf`
- Config (TOML): listen address, data dir, OIDC issuer/client/secret file, allowed subject(s), seller data, tax mode, numbering formats, Typst template dir, font paths
- Secrets only via files (`*_file` options), never inline – fits sops-nix

## Data model

- `customer`: id, number (`K-001`, assigned on create, never reused), name, address lines, email, default hourly rate, note, archived
- `project`: id, customer_id, name, hourly rate (nullable → customer default), archived
- `time_entry`: id, project_id, start, end (null = running), description, billable, invoice_id (null = unbilled)
- `invoice`: id, number (null while draft), customer_id, issue date, due date, service period, status (`draft` | `issued` | `paid` | `cancelled`), totals, snapshot JSON, pdf path, pdf sha256
- `invoice_line`: id, invoice_id, position, description, detail, quantity, unit, unit price
- `sequence`: name, year, last value – for gapless numbering

Rules:
- Only one running timer.
- Invoice numbers are assigned when issuing, not for drafts: format configurable, default `{year}-{seq:03}`, sequence per year, inside one transaction → no gaps.
- Issuing freezes the invoice: snapshot of seller data, customer address, lines and totals is stored; PDF is rendered once and hashed. No edits after issuing.
- Corrections only via a cancellation invoice (Stornorechnung) referencing the original, never by editing or deleting.
- Time entries linked to an issued invoice are locked; cancelling the invoice releases them.

## Typst contract

The app renders `<template_dir>/invoice.typ` with:

```sh
typst compile --root <template_dir> --font-path <fonts> \
  --input data='<json>' <template_dir>/invoice.typ <out.pdf>
```

The template reads `json(bytes(sys.inputs.data))`. JSON shape (version it: `"version": 1`):

```json
{
  "version": 1,
  "kind": "invoice",
  "number": "2026-001",
  "issue_date": "2026-10-25",
  "due_date": "2026-11-08",
  "service_period": "Oktober 2026",
  "seller": { "name": "…", "company": "…", "address": ["…"], "email": "…", "web": "…", "phone": "…",
              "tax_number": "…", "bank": { "name": "…", "iban": "…", "bic": "…" }, "small_business": true, "vat_rate": 19 },
  "customer": { "number": "K-001", "name": "…", "address": ["…"] },
  "lines": [ { "description": "…", "detail": "…", "quantity": 1, "unit": "psch.", "unit_price": "150.00" } ],
  "totals": { "net": "150.00", "vat": "0.00", "gross": "150.00" },
  "cancels": null
}
```

Money as decimal strings, never floats.

- The repo ships a neutral default template (`templates/default/invoice.typ`).
- swilk uses its own design: `~/workspace/mine/gewerbe/corporate-design/typst/` gets an `invoice.typ` entry point that maps this JSON onto the existing `rechnung()` / `stornorechnung()` functions in `lib/vorlagen.typ`. The `firma.toml` values move into `seller` in the app config.

## Phases

### Phase 1 – Foundation and time tracking
- Repo setup: `flake.nix` (dev shell, package, checks), license, README, CI (lint, test, `nix flake check`)
- Config loading, SQLite migrations, OIDC login restricted to configured subject(s), CSRF protection, secure cookies
- Customers and projects: list, create, edit, archive
- Timer (start/stop, one running), manual entries, week view with totals
- Done when: running locally with `nix run`, time can be tracked against a project of a new customer

### Phase 2 – Invoices
- Draft from customer + period: pull unbilled billable entries (grouped per project or per entry), add free lines
- Preview (render without number, watermark "Entwurf"/"Draft")
- Issue: assign number, snapshot, render, store PDF + hash
- Mark paid, cancel via cancellation invoice, download PDF
- Default Typst template + swilk entry point in corporate-design
- Done when: invoice 2026-001 for Genossenschaft Aufwind can be reproduced identically to the current PDF

### Phase 3 – Overview
- Dashboard: running timer, hours this week/month, unbilled hours per customer, open and overdue invoices

### Phase 4 – Deployment (pi5dd)
- NixOS module in the repo (`nixosModules.default`): systemd service, `DynamicUser`, `StateDirectory`, hardening options
- `/etc/nix-config/hosts/pi5dd/apps/compact-company.nix`: enable module, sops secret for OIDC client secret, Caddy vhost `tls internal`, VPN-only like `timetracker.nix`
- Keycloak client `compact-company` in realm `public`
- restic: `sqlite3 .backup` snapshot + `invoices/` (keep 8 years)
- Migrate: import existing customers/invoices from TimeTracker (API or manual), then retire TimeTracker

## Later (not planned yet)

- Customer portal: separate route prefix `/portal`, only that exposed publicly, email magic links with rate limiting, customer sees only own invoices
- Quotes (Angebot) and other documents from corporate-design
- E-invoice (ZUGFeRD) embedding
- Recurring invoices (hosting/maintenance)

## Context (swilk)

- Brand and Typst templates: `~/workspace/mine/gewerbe/corporate-design` (`typst/lib/vorlagen.typ`, `typst/firma.toml`, private `typst/firma.local.toml`)
- Existing invoice: `corporate-design/rechnungen/2026-001_K-001.typ`
- Server config: `/etc/nix-config/hosts/pi5dd/apps/` (pattern: `timetracker.nix`)
- Login: Keycloak `https://user.swilk.eu/realms/public`
- Tax: Kleinunternehmer (§ 19 UStG), invoices per § 34a UStDV
