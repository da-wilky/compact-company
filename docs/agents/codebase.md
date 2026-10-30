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

`internal/app/templates/<page>.html` defines `"title"` and `"content"`. Strings via `{{t "key" args…}}`. Shared markup lives in `templates/partials/*.html`, parsed into every page. A URL with more than one query parameter is built in Go (`url.Values`), never joined in the template.

Page anatomy, top to bottom (`templates_test.go` checks the mechanical parts):

- **Crumb:** pages linked from `layout.html` have none; every other page has exactly one `.crumb`, pointing at its hierarchical parent and labelled with that page's `h1`.
- **Heading:** the `h1` renders `{{template "title" $}}`. New/edit pages read "New X" / "Edit X", the record's identity as a `.hint` subtitle. A page whose crumb names a record also defines `"parent"` (the crumb label) and `"tab"` (`{{template "title" .}} · {{template "parent" .}}`) for `<title>`.
- **Errors:** one `ul.errors role="alert"` directly under the heading holds every error of the page; pages that render invoices use the `render-errors` partial for missing business data and Typst failures.
- **Actions:** "New …" and other page actions are a `p.actions` row under the heading, below the whole `.page-head` when there is a pager.
- **Empty lists:** a `p.hint`: "No X yet." when nothing exists, wording that names the filter when a filter hides everything.
- **Buttons:** a form's main submit is accent with an icon (`.form > button`, `button.primary` elsewhere); destructive buttons and links are `.danger`; no Cancel buttons.

Build it from the shared components in `app.css`:

| Class | For |
|---|---|
| `.crumb` | back link above the `h1`, with `icon-arrow-left` |
| `.page-head` | title row: `h1` (plus `.hint` subtitle) left, `.pager` right; no action pills |
| `.pager` | navigation: `.prev` / `.next` links, a `form.jump` with a date input (`app.js` turns it into the calendar picker), outlined `.pill` shortcuts (no icons) before `.prev` |
| `.filters` | filter row under the header on every list page: one `filter.show` `.label` first, then `filter-seg` toggles (`[]filterOption` built in Go; archived is **Active / All**), `filter-select` value filters (`filterSelect`: auto-submits, first option names the filter, archived marked), page size last. Every URL and hidden input keeps the other filters and drops the cursor |
| `.actions` | row of filled pill links with icons |
| `.status-card` | `aside` beside the table in `.invoice-body`: `.status`, `.due` amount, a `.pill.primary` action, forms, a `.danger` link |
| `.form` | the form card; hints are sibling `p.hint`s linked via `aria-describedby`; a read-only form is a disabled `fieldset` with Save hidden |

Strings that `app.js` needs come from the catalog as `data-*` attributes on `<body>` in `layout.html`.

To preview while tests are red (`nix run` builds with tests and fails), use `go run ./cmd/compact-company serve --config <copy of config.dev.toml>` on another port; 9440 is often taken by the VS Code debug session. Templates and static files are embedded, so restart after each change.

## Add catalog strings

`internal/app/messages/<feature>.toml`, keys prefixed by feature:

```toml
"customers.title" = { en = "Customers", de = "Kunden" }
```

A plain Save, Delete, yes or no uses `common.save` / `common.delete` / `common.yes` / `common.no`; a label that says more ("Delete draft") keeps its own key.

Startup, and so every test, fails if a key is duplicated or lacks `en` or `de`. Language is the user's own, else the default (`a.defaults()`: the stored row with config's `[defaults]` keys laid over it); `Accept-Language` is never read.

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
- Issuing an invoice and sending a quote share `issue.go`: `nextInSequence` (gapless yearly number inside the tx), `commitWithPDF` (stores the PDF and its SHA-256, commits, leaves no file on failure) and `serveStoredPDF` for the download.

## Typst guards

Every field that stores Typst markup (quote sections, snippets) uses the guards of ADR 0007 in `typst_guards.go`:

- **Lint:** `lintTypst(body) []typstFinding`, pure Go. The field is the `typst-body` partial (`partials/typst_body.html`, fed `newTypstBody(body)`): its textarea posts to `POST /typst/lint` while typing, and each finding's button posts `escape=<Pos>` there to escape it. `typst-cheat-sheet` goes next to the form.
- **Compile check:** `checkTypst(ctx, body) (*typstFailure, error)` evaluates the body alone as `quote.typ` does; a non-nil failure names the first paragraph that fails on its own, the error is for typst not running. Show it with the `typst-failure` partial. `checkSections` runs it over a quote's sections. A quote section saves even when it fails (sending refuses it); a snippet doesn't save at all, since nothing else would stop it reaching every quote.

## Todo boards

- **Board:** every board is the `board` partial (`partials/board.html`) fed by `loadBoard` in `todos.go`: all statuses as columns, cards in the one board order (`boardOrder`; new todos get `firstStatus`, `overdue` decides the badge). Its `where` takes plain `?` placeholders only; `loadBoard` panics on `?1`, which SQLite would number against its own `?`.
- **Status change:** whatever triggers it, it posts `status` and `return` to `POST /todos/{id}/status`, which redirects to `return` (a board page, checked by `boardReturn`); htmx swaps `#board` out of the page that redirect lands on. A new board page passes its own URL as `return` and adds its prefix to `boardReturn`. The todo edit page also sets the status.
- **Card status menu and drag:** the card's status `<select>` is what posts. `statusMenus` in `app.js` hides it behind a popover menu, and dropping a card on another column (`data-status`) picks that status in it, so menu, drag and the no-JS dropdown send the same request. Touch screens have no native drag, so the menu stays. A refused change resets the select and `app.js` shows the error page's heading above the board.
- **Archived:** a card of an archived project or customer has no status menu, drag or timer, and the handler refuses its status change (422); archiving leaves statuses as they are.
- **Todos page** (`todos_page.go`): sets `ShowProjects` so cards name their project. `assignee=me` resolves to the viewer, so `/todos?assignee=me` is the link to one's own todos. "New todo" (`/todos/new`) picks the project, preselecting the project filter.

The status list and working status are edited in Settings (`statuses.go`); every change goes through `changeStatuses`, which rolls back with a message if the first or the working status ends up done.

Time on a todo: an entry's project is always its todo's project. Triggers on `time_entry` (`0012_todo.sql`) hold it for every write: setting a todo of another project aborts, changing the project clears the todo, deleting a todo clears it on its entries. So a write that sets a todo sets its project in the same statement, and moving a todo updates the todo before its entries. `openTodoProject` in `todos.go` resolves a todo that may receive new time (not done) to its project; `startWork` is the move to the working status. `POST /timers` takes `project=todo:<id>` for a todo, plus a board's `return` for plain forms. The entry form posts `project` and `todo` side by side: `validateEntry` lets a newly picked todo set the project, else a project other than the todo's clears the todo.

## Mass actions and confirmation

Selecting rows and acting on all of them (entries, invoices, customers, projects) is one mechanism; a new list or action plugs into it.

- **Server:** `massWrite` in `selection.go` takes a `massAction` (row loader, blocking rule, write, list renderer). It writes all rows or none: unknown id → 404; a blocked row or refused write → 422 with the list re-rendered, the selection kept and `selectionRefusal.Blocked` named. The page embeds `selectionRefusal[T]`; action URLs are built in Go and carry the list's query.
- **Markup** (`customers.html` is the smallest example): a hidden `form.selection` with `hx-post`, `hx-target`/`hx-select` on the list wrapper; `input[data-select-all]`; `.selection-actions` holding `[data-count]` and one button per action (`formaction`, optional `data-only-state`; a delete adds `data-confirm-one`/`data-confirm-many` with `{n}` and `data-confirm-ok`). Row checkboxes: `name="id"`, `form=` the selection's id, `data-state` when actions depend on it. Only actionable rows get a checkbox; the table carries `class="selectable"`, refusals render as `.errors.selection-blocked`.
- **Confirmation:** only for hard-to-reverse actions (delete, undo paid). Any form or submit button with `data-confirm` opens the `dialog.confirm` in `layout.html` (`confirmForms` in `app.js`); `data-confirm-ok` names the button, `data-confirm-neutral` makes it accent instead of danger.

## Tests

Only through the `http.Handler`, via the harness. That is the seam for `/tdd` too, so start each slice with a red test through it:

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

A link's `aria-current` sits between its `href` and `>`, so match `href="…">Label</a>` on a link that isn't the current one. Go-built URLs render with sorted keys and `&amp;`: `href="/entries?day=…&amp;project=…"`.

## Checks

While working, run `nix run .#check`: formatting, lint, the markup-assertion, table-wrap, CSS-rule and template-URL checks and tests, on the working tree. Run it as its own Bash call and write files with Write/Edit: worktree-isolated agents get chained shell (`&&`, `;`, `sh -c`, heredocs) refused.

`nix flake check -L` is what CI runs: the same checks from git-tracked files only, so `git add` new files first.

After any template or CSS change, run `nix run .#ui-check` (Linux): it seeds a throwaway instance, renders every page in English and German at 1280 and 390 px in light and dark, fails on horizontal overflow, invisible or cut-off text and elements that take focus without showing it, and prints a directory of screenshots. Each run takes a free port, so parallel runs are fine. Look at the screenshots of the pages you touched; the check can't judge alignment or wrapping. New pages go into `pages` in `scripts/ui_check.py`. To iterate on a few pages, pass their paths: `nix run .#ui-check -- /invoices/1` (seeded ids are listed in the script's docstring), or `-- --changed main` for the pages whose template changed; read the phone shots first, they are the smallest.

## Nix

- After changing `go.mod`/`go.sum`: set `vendorHash = pkgs.lib.fakeHash;`, run `nix build`, paste the `got:` hash.
- Keep `subPackages` off `buildGoModule`: it would limit the check phase's `go test` and skip `internal/app`.
