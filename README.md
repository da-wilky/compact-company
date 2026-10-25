# compact-company

Customers, time tracking and invoices for small businesses — and nothing else. A single self-hosted Go binary backed by SQLite, with server-rendered HTML and htmx. One instance serves one business; several users can log in and track time.

Status: Phase 1 (foundation and time tracking) is in progress. See `PLAN.md`.

## Running

With Nix:

```sh
nix run                                   # dev mode on 127.0.0.1:8080, data in ./data
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
data_dir = "/var/lib/compact-company"  # required; holds app.db
customer_number_format = "K-{seq:03}"  # default

[auth]
mode = "oidc"                      # "oidc" or "none"
session_idle_timeout = "336h"      # default: 14 days
issuer = "https://id.example.com/realms/main"
client_id = "compact-company"
client_secret_file = "/run/secrets/compact-company-oidc"
allowed_subjects = ["8c1f…"]       # OIDC subjects admitted to log in

[defaults]                         # applied to new users and customers
currency = "EUR"                   # ISO 4217
language = "en"                    # "en" or "de"; also used before login
timezone = "Europe/Berlin"         # default: UTC
parallel_entries = false
```

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
