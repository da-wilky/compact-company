# No frontend framework

The UI stays server-rendered with htmx: one hand-written CSS file, one embedded font and a small `app.js` for what htmx can't do (live timer clock, keyboard shortcuts), with no build step. Svelte was considered and rejected: tests run only through the `http.Handler` and would miss client-side logic, a Node build adds a second vendor hash to the Nix build, the TOML catalog would have to reach the client, and the app's forms and tables hold almost no client state.
