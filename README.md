# compact-company

A self-hosted business suite for one German business: todos and timers, quotes and invoices with e-invoicing and your own Typst layouts, receipts, bank import and double-entry books. Rust and Svelte, SQLite, one binary.

Status: being rebuilt from scratch; see `PLAN.md` and `docs/adr/`. The previous Go version lives on the `old-go-version` branch.

## Running

```sh
nix run                      # serves on 127.0.0.1:9440, data in ./data
nix run . -- --config config.toml
nix build .#image            # OCI image on 0.0.0.0:9440 with data in /data; load with `docker load < result`
```

On first start the server prints a one-time setup link; whoever opens it creates the owner with a passkey.

Every config key is optional:

```toml
listen = "127.0.0.1:9440"
public_url = "http://localhost:9440"   # the address browsers use; passkeys are bound to its host
data_dir = "data"              # database, and stored files unless S3 is set
trusted_proxies = ["127.0.0.1"] # reverse proxies whose X-Forwarded-For names the client, for rate limits
typst_packages = "https://packages.typst.org/"  # where approved Typst packages are downloaded from

[storage.s3]                   # S3-compatible bucket for stored files
bucket = "compact-company"
region = "eu-central-1"
endpoint = "https://s3.example.com"
access_key_id = "…"
secret_access_key_file = "/run/secrets/s3"   # secrets come only from files

[oidc]                         # optional sign-in through an identity provider; it never creates users
issuer = "https://id.example.com/realms/example"
client_id = "compact-company"  # redirect URI: <public_url>/api/oidc/callback
client_secret_file = "/run/secrets/oidc"
name = "Keycloak"              # label of the sign-in button

[smtp]                         # for sending documents by email
host = "smtp.example.com"
port = 587
security = "starttls"          # or "tls" (port 465), or "none" for a relay on this host
username = "rechnung@example.com"
password_file = "/run/secrets/smtp"
from = "Example GmbH <rechnung@example.com>"
bcc = "archiv@example.com"     # optional copy of everything sent

[business]                     # each key set here is fixed; the app shows it read-only
name = "…"
street = "…"
postcode = "…"
city = "…"
tax_number = "…"
vat_id = "…"
small_business = true
currency = "EUR"               # currency new customers start with
invoice_number_format = "RE-{yyyy}-{nnn}"  # also quote_, order_confirmation_, delivery_note_, acceptance_report_
```

## NixOS

```nix
{
  imports = [ compact-company.nixosModules.default ];
  services.compact-company = {
    enable = true;
    settings = {
      public_url = "https://cc.example.com/";
      trusted_proxies = [ "127.0.0.1" ];  # the reverse proxy on this host
      business.name = "Example GmbH";
      # any *_file setting, such as oidc.client_secret_file, reaches the service as a systemd credential
    };
  };
}
```

The service listens on `127.0.0.1:9440` for a reverse proxy and keeps its data in `/var/lib/compact-company`; `journalctl -u compact-company` shows the setup link. Sign-in, setup, invitation and OIDC starts are limited to a burst of 20 and then 10 a minute per client address (per /64 for IPv6); without `trusted_proxies` every request through the proxy counts as one client.

## Developing

```sh
nix develop
npm --prefix web ci && npm --prefix web run build   # the server embeds web/build at compile time
cargo run -- serve
npm --prefix web run dev     # client with hot reload, proxies /api to the server
nix flake check              # fmt, clippy, tests, svelte-check, NixOS VM test (x86_64)
nix run .#ui-check           # every screen at desktop and phone width, light and dark, into ui-check/
```

`ui-check` sets up a fresh instance with a virtual passkey and fails on horizontal overflow or browser errors.

Licensed under AGPL-3.0-or-later.
