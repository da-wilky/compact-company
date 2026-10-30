# compact-company

Customers, time tracking and invoices for small businesses — and nothing else. A single self-hosted Go binary backed by SQLite, with server-rendered HTML and htmx. One instance serves one business; several users can log in and track time.

Status: Phase 1 (foundation and time tracking) is in progress. See `PLAN.md`.

## Running

With Nix:

```sh
nix run                                   # dev mode on 127.0.0.1:9440, data in ./data
nix run . -- serve --config config.toml   # any other config
```

Without Nix, build with Go 1.25 or newer:

```sh
go build ./cmd/compact-company
./compact-company serve --config config.toml
```

The binary embeds its templates, CSS, htmx, migrations and timezone data. The database is migrated automatically on startup; back up `app.db` before upgrading, as there are no down migrations.

## Configuration

One TOML file. Unknown keys are a startup error. Secrets are only read from files (`*_file` keys), never inline.

```toml
listen = "127.0.0.1:8080"          # default
public_url = "https://time.example.com"  # required in oidc mode; redirect URI is <public_url>/auth/callback
data_dir = "/var/lib/compact-company"  # required; holds app.db
customer_number_format = "K-{seq:03}"  # default

[auth]
mode = "oidc"                      # "oidc" or "none"
session_idle_timeout = "336h"      # default: 14 days
issuer = "https://id.example.com/realms/main"
client_id = "compact-company"
client_secret_file = "/run/secrets/compact-company-oidc"
allowed_subjects = ["8c1f…"]       # OIDC subjects admitted to log in

[defaults]                         # optional; each key set here overrides the settings page and shows read-only there
currency = "EUR"                   # ISO 4217
language = "en"                    # "en" or "de"; also used before login
timezone = "Europe/Berlin"         # default: UTC
parallel_entries = false

[invoice]                          # defaults for new drafts
number_format = "{year}-{seq:03}"  # default; {year} and {seq} once each; no "/" or ".."
payment_days = 14                  # default; positive
rounding_minutes = 0               # default; 0, 5, 10, 15 or 30
template_dir = "/etc/compact-company/invoice-template"  # optional; default: bundled template
font_paths = ["/etc/compact-company/fonts"]             # optional; extra Typst font dirs

[quote]
number_format = "A-{year}-{seq:03}"  # default; {year} and {seq} once each; no "/" or ".."
validity_days = 30                 # default; positive; new quotes are valid until today plus this
```

The template contract (data, invocation, fonts) is in [templates/README.md](templates/README.md).

The business data printed on invoices is edited under Settings → Business. To manage it from config instead, set `[business]`, `business_file`, or both; the settings page then shows it read-only. `business_file` is a TOML file with the same keys as `[business]` (for sops secrets); its keys override the same keys in `[business]`.

```toml
business_file = "/run/secrets/compact-company-business"  # optional

[business]
name = "Jane Doe"
company = "Doe Consulting"
address = ["Main Street 1", "12345 Berlin"]
email = "jane@example.com"
web = "example.com"
phone = "+49 30 123456"
tax_number = "12/345/67890"
tax_mode = "small_business"        # "small_business" (§ 19 UStG, default) or "standard"
vat_rate = 19                      # integer percent, 0 to 100; used in "standard" mode

[business.bank]
name = "Example Bank"
iban = "DE02120300000000202051"
bic = "BYLADEM1001"
```

Invoices can't be previewed or issued while name, address or tax number is missing.

`auth.mode = "none"` logs every visitor in as a fixed `dev` user. It is refused unless `listen` is a loopback address.

## Development

```sh
nix develop          # Go, gopls, golangci-lint, sqlite
go test ./...
golangci-lint run
nix flake check      # what CI runs: tests and lint
```

## License

[AGPL-3.0](LICENSE)
