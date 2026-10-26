# Codebase conventions

## Packages

```
cmd/compact-company/main.go   serve --config <file>
internal/config/config.go     Config struct; Load = decode + unknown-key check + validate()
internal/db/db.go             SQLite (modernc, WAL) + embedded migrations
internal/db/migrations/       NNNN_name.sql
internal/app/                 the http.Handler; one file per feature
  templates/                  layout.html + one template per page
  messages/<feature>.toml     translation catalog
  static/                     app.css, app.js, htmx.min.js, favicon.svg, fonts/; served at /static/
  harness_test.go             test harness (package app_test)
  export_test.go              test-only exports
```

## Add a route

In `internal/app/<feature>.go`:

```go
func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /customers", a.authed(a.customerList))
	})
}
```

- `a.authed` for anything needing a user; `currentUser(r)` returns the `*User`.
- Render with `a.render(w, r, status, "<page>", data)`; the template gets `.Lang`, `.User`, `.Data`.
- Internal errors: `a.serverError(w, r, err)`.

## Add a page

`internal/app/templates/<page>.html` defines `"title"` and `"content"`. Strings via `{{t "key" args…}}`.

Build the page from the shared components in `app.css`:

| Class | For |
|---|---|
| `.page-head` | title row: `h1` (plus `.hint` subtitle or a `.pill` action) left, `.pager` right |
| `.pager` | navigation: `.prev` / `.next` links, a `form.jump` with a date input (`app.js` turns it into the calendar picker), `.pill` shortcuts before `.prev` |
| `.filters` | filter row under `.page-head`: `.label`, `.seg` toggle (`aria-current` marks the active option), removable `.chip` |
| `.actions` | row of pill links |
| `.form`, `.settings` | form cards; a `.hint` inside a label stays muted |

Strings that `app.js` needs come from the catalog as `data-*` attributes on `<body>` in `layout.html`.

To preview while tests are red (`nix run` builds with tests and fails), use `go run ./cmd/compact-company serve --config <copy of config.dev.toml>` on another port; 9440 is often taken by the VS Code debug session. Templates and static files are embedded, so restart after each change.

## Add catalog strings

`internal/app/messages/<feature>.toml`, keys prefixed by feature:

```toml
"customers.title" = { en = "Customers", de = "Kunden" }
```

Startup, and so every test, fails if a key is duplicated or lacks `en` or `de`. Language is the user's setting, else `defaults.language`; `Accept-Language` is never read.

## Add a migration

`internal/db/migrations/NNNN_<name>.sql`, applied in one transaction with `PRAGMA user_version`. Numbers must run 1..n without gaps, or startup fails. Parallel branches pick the same next number: whoever merges second renumbers.

Tables are `STRICT`, ids `INTEGER PRIMARY KEY`, timestamps INTEGER unix seconds UTC, booleans INTEGER. No down migrations.

`ADD COLUMN` checks a new CHECK against existing rows, so a constraint that needs a backfill becomes a trigger (`0008_invoice_timezone.sql`). Rebuilding a table drops its triggers: recreate them.

## Invoice state

Reuse these instead of re-deriving them:

- Status is derived, never stored: `invoiceStatus` (SQL, invoice aliased `i`) in `issue.go`.
- What's still owed: `amountDue` in `open_invoices.go`.
- A time entry is claimed via `time_entry.invoice_line_id`; its invoice is a draft while `number IS NULL`.
- Invoice dates are ISO dates in the customer's timezone; resolve timezones per query with `locationCache`.
- Week and month windows: `mondayOf` in `week.go`.

## Tests

Only through the `http.Handler`, via the harness:

```go
ta := newTestApp(t, devConfig)              // or oidcConfig(t)
alice := ta.loginAs("alice")
res := alice.get("/path")                   // res.wantStatus, res.wantBody, res.wantNoBody
res.wantRow(t, "website", "yes", "85.00 EUR") // a table row's cells as the user reads them
res.in(t, "Drafts").rows()                  // only the section under that h2, so other tables don't leak in
wantInOrder(t, res.text(), "Mon 2026-10-26 1:30", "later") // text() strips tags
res = alice.postForm("/path", url.Values{})
ta.clock.Advance(time.Hour)                 // starts Monday 2026-10-26 09:00 UTC
ta.restart()                                // reopen the same data dir
wantStartupError(t, toml, "substring")
```

The harness injects `data_dir`; test configs leave it out. OIDC tests use the stub issuer in `issuer_test.go`.

## Checks

While working, run `nix run .#check`: formatting, lint, the markup-assertion and table-wrap checks and tests, on the working tree. Run it as its own Bash call and write files with Write/Edit: worktree-isolated agents get chained shell (`&&`, `;`, `sh -c`, heredocs) refused.

`nix flake check -L` is what CI runs: the same checks from git-tracked files only, so `git add` new files first.

After any template or CSS change, run `nix run .#ui-check` (Linux): it seeds a throwaway instance, renders every page in English and German at 1280 and 390 px in light and dark, fails on horizontal overflow or invisible text, and prints a directory of screenshots. Look at the screenshots of the pages you touched; the check can't judge alignment or wrapping. New pages go into `pages` in `scripts/ui_check.py`. To iterate on a few pages, pass their paths: `nix run .#ui-check -- /invoices/1` (seeded ids are listed in the script's docstring).

## Nix

- After changing `go.mod`/`go.sum`: set `vendorHash = pkgs.lib.fakeHash;`, run `nix build`, paste the `got:` hash.
- Keep `subPackages` off `buildGoModule`: it would limit the check phase's `go test` and skip `internal/app`.
