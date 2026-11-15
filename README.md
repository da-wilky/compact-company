# Compact Company

**A self-hosted business suite for one German business**: todos and timers, quotes and invoices with e-invoicing and your own Typst layouts, receipts, bank import, double-entry books, tax figures and GoBD-compliant retention.

One Rust binary with an embedded Svelte app and SQLite. Runs on a Raspberry Pi.

[![CI](https://github.com/da-wilky/compact-company/actions/workflows/ci.yml/badge.svg)](https://github.com/da-wilky/compact-company/actions/workflows/ci.yml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)

- [Features](#features)
- [Quick start](#quick-start)
- [Deployment](#deployment)
- [Configuration](#configuration)
- [Backup and restore](#backup-and-restore)
- [Document layouts](#document-layouts)
- [API](#api)
- [Development](#development)
- [Documentation](#documentation)

## Features

**Work**
- Customers with permanent numbers, projects, archiving
- Hourly rates per customer, per project and per activity type
- Todos as list and board, with priority, due date, assignee and comments
- Timers on a project or todo, several running at once; manual entries and a week view

**Documents**
- Quote, order confirmation, invoice, cancellation, dunning letter, delivery note, timesheet, acceptance report
- Invoices from unbilled time, grouped and rounded as you choose; recurring invoices; partial and final invoices
- E-invoices from the start: ZUGFeRD and XRechnung (EN 16931), embedded in PDF/A-3
- The mandatory details of § 14 Abs. 4 UStG are checked before issuing
- Sending by email, with a log per document

**Books**
- Journal on SKR03 or SKR04: invoices, payments, receipts, bank transactions and depreciation post to it; corrections only by reversal
- Kleinunternehmer (§ 19 UStG), cash basis (Ist) or accrual basis (Soll)
- Receipts as PDF or photo; incoming ZUGFeRD and XRechnung are read automatically
- Bank import (CAMT.053 and CSV) with matching suggestions and partial payments
- Open items, dunning, fixed assets with depreciation

**Tax and exports**
- VAT return and EÜR figures, each traceable to its postings
- DATEV export, deadlines as an ICS feed and email reminders
- Year-end archive with checksums; closed years can't be changed

**Retention and backup**
- A readable copy of every document in a Nextcloud folder (WebDAV)
- Daily encrypted backups to S3, and a restore command
- Deletion after the statutory retention periods, and anonymisation after ten years
- Process documentation (GoBD Verfahrensdokumentation), generated where the app knows the answer

**Access**
- Passkeys, optional sign-in through OIDC (such as Keycloak)
- Roles owner, staff and accountant
- REST API with scoped tokens

Deliberately left out: several businesses per instance, reverse charge and EU cases, balance sheets for UG/GmbH, ELSTER transmission, payroll and cash book.

## Quick start

With [Nix](https://nixos.org):

```sh
nix run github:da-wilky/compact-company
```

This serves on <http://localhost:9440> and keeps its data in `./data`. On the first start the server prints a one-time setup link; whoever opens it creates the owner account with a passkey.

## Deployment

Compact Company is a single process with its data in one directory: the SQLite database, and stored files unless they go to S3. Put it behind a reverse proxy with TLS, since passkeys need HTTPS outside `localhost`.

### NixOS (recommended)

```nix
{
  inputs.compact-company.url = "github:da-wilky/compact-company";

  outputs = { nixpkgs, compact-company, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      modules = [
        compact-company.nixosModules.default
        {
          services.compact-company = {
            enable = true;
            settings = {
              public_url = "https://company.example.com";
              business.name = "Example GmbH";
              smtp = {
                host = "smtp.example.com";
                username = "rechnung@example.com";
                password_file = "/run/secrets/smtp";
                from = "Example GmbH <rechnung@example.com>";
              };
            };
          };

          services.caddy = {
            enable = true;
            virtualHosts."company.example.com".extraConfig = "reverse_proxy 127.0.0.1:9440";
          };
        }
      ];
    };
  };
}
```

- `settings` is the [configuration](#configuration) as Nix.
- Each key ending in `_file` reaches the service as a systemd credential, so secret files can stay readable by root only.
- The service listens on `127.0.0.1:9440` and trusts a reverse proxy on the same host (`trusted_proxies`).
- It keeps its data in `/var/lib/compact-company` and runs as a sandboxed dynamic user.
- `journalctl -u compact-company` shows the setup link on the first start.

### Container

```sh
nix build github:da-wilky/compact-company#image
docker load < result
docker run -d -p 9440:9440 -v compact-company:/data compact-company:latest
```

The image listens on `0.0.0.0:9440` and keeps its data in the volume `/data`. To use your own config, mount it and pass it on; it then needs `listen` and `data_dir` too:

```sh
docker run -d -p 9440:9440 -v compact-company:/data -v "$PWD/config.toml:/config.toml:ro" \
  compact-company:latest serve --config /config.toml
```

### Other Linux hosts

```sh
nix build github:da-wilky/compact-company
./result/bin/compact-company serve --config /etc/compact-company/config.toml
```

### Behind a reverse proxy

- Set `public_url` to the address browsers use; passkeys are bound to its host.
- Sign-in, setup, invitations and OIDC starts are rate-limited to a burst of 20, then 10 a minute per client address (per /64 for IPv6).
- List your proxy in `trusted_proxies`: without it, every request through the proxy counts as one client.

### Updates

Database migrations run on start. Take a backup before updating, for example `sqlite3 compact-company.db ".backup before-update.db"` in the data directory.

## Configuration

A TOML file passed with `--config`. Every key is optional; without a file, the defaults apply. Secrets come only from files (`*_file`).

```toml
listen = "127.0.0.1:9440"
public_url = "http://localhost:9440"   # the address browsers use; passkeys are bound to its host
data_dir = "data"                      # database, and stored files unless S3 is set
trusted_proxies = ["127.0.0.1"]        # reverse proxies whose X-Forwarded-For names the client
typst_packages = "https://packages.typst.org/"  # where approved Typst packages come from

[storage.s3]                   # stored files in an S3-compatible bucket instead of data_dir
bucket = "compact-company"
region = "eu-central-1"
endpoint = "https://s3.example.com"
access_key_id = "…"
secret_access_key_file = "/run/secrets/s3"

[oidc]                         # sign-in through an identity provider; it never creates users
issuer = "https://id.example.com/realms/example"
client_id = "compact-company"  # redirect URI: <public_url>/api/oidc/callback
client_secret_file = "/run/secrets/oidc"
name = "Keycloak"              # label of the sign-in button

[smtp]                         # sending documents, reminders and alerts by email
host = "smtp.example.com"
port = 587
security = "starttls"          # or "tls" (port 465), or "none" for a relay on this host
username = "rechnung@example.com"
password_file = "/run/secrets/smtp"
from = "Example GmbH <rechnung@example.com>"
bcc = "archiv@example.com"     # a copy of everything sent

[mirror]
language = "de"                # folder and file names; else the business language, else English

[mirror.webdav]                # a readable copy of every document, such as a Nextcloud folder
url = "https://cloud.example.com/remote.php/dav/files/user/Firma"
username = "user"
password_file = "/run/secrets/nextcloud"   # a Nextcloud app password

[backup]                       # daily encrypted snapshots of the database and every file
recipient = "age1…"            # age public key; keep its identity off the server
time = "03:00"                 # in the business's timezone
keep_daily = 7                 # the newest snapshot of each of this many days is kept
keep_monthly = 12              # and of each of this many months
prefix = "compact-company"     # key prefix in the bucket

[backup.s3]                    # a bucket of its own, not the storage one
bucket = "backups"
region = "eu-central-1"
endpoint = "https://s3.example.com"
access_key_id = "…"
secret_access_key_file = "/run/secrets/backup-s3"

[business]                     # each key set here is fixed; the app shows it read-only
name = "Example GmbH"
street = "Musterstraße 1"
postcode = "12345"
city = "Musterstadt"
tax_number = "…"
vat_id = "…"
taxation = "small_business"    # or "cash" (Ist) or "accrual" (Soll)
currency = "EUR"               # the currency new customers start with
invoice_number_format = "RE-{yyyy}-{nnn}"  # likewise quote_, order_confirmation_, delivery_note_,
                                           # acceptance_report_ and dunning_number_format
```

Everything under `[business]` can also be set in the app instead; the config only pins it.

## Backup and restore

With `[backup]` set, the app backs up every day at `time`. Each snapshot holds a copy of the database and every stored file, encrypted with [age](https://age-encryption.org) to `recipient`. Older snapshots are rotated by `keep_daily` and `keep_monthly`. The backup page shows the last run and has a "back up now" button. A failed backup is retried after an hour, mails the owner and shows on the overview.

Create the key once, on a machine other than the server:

```sh
age-keygen -o backup.key     # prints the public key for `recipient`
```

Keep `backup.key` safe and off the server. It is the only way to read the backups.

To restore, use a fresh instance whose config has the same `[backup]` section and an empty data directory (and an empty file store, if S3 is used):

```sh
compact-company restore --config config.toml --identity backup.key             # lists the snapshots
compact-company restore --config config.toml --identity backup.key --snapshot 20261114T020000.000Z
```

The restore checks every file and every year archive against its checksum. Then start the server as usual.

## Document layouts

Every document is rendered by a [Typst](https://typst.app) layout. A built-in one is used until you upload your own and pick it per document type.

A layout is a `layout.typ` defining `schema` and `render(data)`, plus the images it reads under `assets/` and the fonts it uses. Two commands help while writing one:

```sh
compact-company sample --layout ./my-layout --kind invoice > invoice.pdf   # also: partial, final, --language en, --small-business
compact-company einvoice --kind invoice --xrechnung > invoice.xml          # sample e-invoice XML for a validator
```

## API

Everything the app does goes through a REST API under `/api`. `compact-company openapi` prints its OpenAPI description; it is also checked in as [`web/openapi.json`](web/openapi.json).

For scripts, each user creates tokens under Account → API tokens. A token acts as its user, within its scopes: `work:read`, `work:write`, `documents:read`, `documents:write` and `books:read`.

```sh
curl -H "Authorization: Bearer $TOKEN" https://company.example.com/api/customers
```

Tokens never issue, send or book: those steps stay with a person.

## Development

```sh
nix develop                                           # Rust, Node and every tool
npm --prefix web ci && npm --prefix web run build     # the server embeds web/build at compile time
cargo run -- serve                                    # http://localhost:9440, data in ./data
npm --prefix web run dev                              # the app with hot reload, proxying /api to the server
```

Checks:

```sh
nix flake check     # fmt, clippy, tests, svelte-check, e-invoice validation (KoSIT), NixOS VM test
nix run .#ui-check  # every screen at desktop and phone width, light and dark, into ui-check/
```

`ui-check` sets up a fresh instance with a virtual passkey and fails on horizontal overflow or browser errors. CI runs `nix flake check` on x86_64 and aarch64 Linux.

## Documentation

- [`PLAN.md`](PLAN.md): scope, stack and phases
- [`docs/adr/`](docs/adr): the decisions and why they were made
- [`GLOSSARY.md`](GLOSSARY.md): the domain terms, in English and German
- [`docs/legal-sources.md`](docs/legal-sources.md): the legal texts the rules are based on

## License

[AGPL-3.0-or-later](LICENSE)
