# compact-company

A self-hosted business suite for one German business: todos and timers, quotes and invoices with e-invoicing and your own Typst layouts, receipts, bank import and double-entry books. Rust and Svelte, SQLite, one binary.

Status: being rebuilt from scratch; see `PLAN.md` and `docs/adr/`. The previous Go version lives on the `old-go-version` branch.

## Running

```sh
nix run                      # serves on 127.0.0.1:9440
nix run . -- --listen 0.0.0.0:9440
nix build .#image            # OCI image, load with `docker load < result` or `podman load < result`
```

## Developing

```sh
nix develop
npm --prefix web ci && npm --prefix web run build   # the server embeds web/build at compile time
cargo run -- serve
npm --prefix web run dev     # client with hot reload, proxies /api to the server
nix flake check              # fmt, clippy, tests, svelte-check
```

Licensed under AGPL-3.0-or-later.
