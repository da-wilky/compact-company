# Coding standards

Judgement calls for review. Mechanical rules live in `.golangci.yml` and `nix flake check`.

- Money is stored as integer minor units of its currency. Any change to a currency converts the stored amounts that depend on it (half-up).
- Tests exercise behaviour only through the app's `http.Handler` (see `docs/agents/codebase.md`).
- Every user-facing string goes through the catalog, in both `en` and `de`.
- Comments explain a non-obvious why; names carry the rest.
