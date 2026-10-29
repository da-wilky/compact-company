# Coding standards

Judgement calls for review. Mechanical rules live in `.golangci.yml` and `nix flake check`; UI layout breakage in `nix run .#ui-check`.

- Money is stored as integer minor units of its currency. Any change to a currency converts the stored amounts that depend on it (half-up).
- Tests exercise behaviour only through the app's `http.Handler` (see `docs/agents/codebase.md`).
- Test assertions target what a user relies on (text, links, form values); layout markup (classes, icons, wrappers) stays out of them. `scripts/markup_assertions.sh` catches the literal cases; judge the rest, and each `// markup is the behaviour` exemption.
- A field that becomes optional (empty or NULL where it never was) is handled at every reader: templates, handlers, queries and the template contract.
- Every action the UI withholds (hidden form, missing button) is also refused by its handler, with a test that posts it directly.
- Every user-facing string goes through the catalog, in both `en` and `de`, using the `GLOSSARY.md` terms in each language; a plain Save or Delete uses the `common.*` key.
- Comments explain a non-obvious why; names carry the rest.
- UI meets WCAG AA: text 4.5:1, form-field and control borders 3:1, visible focus on every interactive element. Colours come from the `:root` tokens in `app.css`, in both light and dark.
- Every page follows the page anatomy and shared components in `docs/agents/codebase.md`; new styles hang off a class the markup opts into.
- A behaviour a second place needs goes through the one mechanism that already does it (mass actions, confirmation, invoice state: see `docs/agents/codebase.md`); a new shared mechanism is documented there in the same change.
