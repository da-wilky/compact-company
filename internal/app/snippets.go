package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /snippets", a.authed(a.snippetList))
		mux.HandleFunc("GET /snippets/new", a.authed(a.snippetNew))
		mux.HandleFunc("POST /snippets", a.authed(a.snippetCreate))
		mux.HandleFunc("GET /snippets/{id}", a.authed(a.snippetEdit))
		mux.HandleFunc("POST /snippets/{id}", a.authed(a.snippetUpdate))
		mux.HandleFunc("POST /snippets/{id}/delete", a.authed(a.snippetDelete))
		mux.HandleFunc("POST /quotes/{id}/snippets", a.authed(a.quoteSnippetInsert))
	})
}

// snippetFromPath writes 404 or 500 itself when ok is false.
func (a *App) snippetFromPath(w http.ResponseWriter, r *http.Request) (snippet, bool) {
	var s snippet
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err == nil {
		err = a.db.QueryRow(`SELECT id, name, language, heading, body FROM snippet WHERE id = ?`, id).
			Scan(&s.ID, &s.Name, &s.Language, &s.Heading, &s.Body)
	}
	if err == nil {
		return s, true
	}
	var numErr *strconv.NumError
	if errors.Is(err, sql.ErrNoRows) || errors.As(err, &numErr) {
		a.notFound(w, r)
	} else {
		a.serverError(w, r, err)
	}
	return s, false
}

type snippet struct {
	ID                            int64
	Name, Language, Heading, Body string
}

func (s snippet) LanguageName() string { return languageNames[s.Language] }

// snippets lists what query selects: id, name, language and heading, without the body.
func (a *App) snippets(query string, args ...any) ([]snippet, error) {
	rows, err := a.db.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var found []snippet
	for rows.Next() {
		var s snippet
		if err := rows.Scan(&s.ID, &s.Name, &s.Language, &s.Heading); err != nil {
			return nil, err
		}
		found = append(found, s)
	}
	return found, rows.Err()
}

func (a *App) snippetList(w http.ResponseWriter, r *http.Request) {
	snippets, err := a.snippets(`SELECT id, name, language, heading FROM snippet ORDER BY language, name, id`)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "snippets", snippets)
}

// quoteSnippetInsert copies a snippet in the draft's language to the end of its sections.
func (a *App) quoteSnippetInsert(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	res, err := a.db.Exec(`INSERT INTO quote_section (quote_id, position, heading, body)
		SELECT q.id, (SELECT ifnull(max(position), 0) + 1 FROM quote_section WHERE quote_id = q.id), s.heading, s.body
		FROM quote q JOIN snippet s ON s.language = q.language WHERE q.id = ? AND q.number IS NULL AND s.id = ?`,
		q.ID, r.PostFormValue("snippet"))
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	n, err := res.RowsAffected()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if n == 0 {
		a.renderQuote(w, r, http.StatusUnprocessableEntity, q, []string{"snippets.error.language"})
		return
	}
	http.Redirect(w, r, quotePath(q.ID), http.StatusSeeOther)
}

type snippetPage struct {
	Snippet   snippet
	Languages []languageOption
	Errors    []string
	Failure   *typstFailure
}

func (p snippetPage) Typst() typstBody { return newTypstBody(p.Snippet.Body) }

func (a *App) snippetNew(w http.ResponseWriter, r *http.Request) {
	d, err := a.defaults()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "snippet", snippetPage{Snippet: snippet{Language: d.Language}, Languages: languageOptions()})
}

// snippetForm keeps the body as typed: Typst gives whitespace meaning. It writes 422 itself when ok is false.
func (a *App) snippetForm(w http.ResponseWriter, r *http.Request, id int64) (snippet, bool) {
	s := snippet{
		ID:       id,
		Name:     strings.TrimSpace(r.PostFormValue("name")),
		Language: r.PostFormValue("language"),
		Heading:  strings.TrimSpace(r.PostFormValue("heading")),
		Body:     r.PostFormValue("body"),
	}
	p := snippetPage{Snippet: s, Languages: languageOptions()}
	if s.Name == "" {
		p.Errors = append(p.Errors, "snippets.error.name")
	}
	if !supportedLanguage(s.Language) {
		p.Errors = append(p.Errors, "quotes.error.language")
	}
	if strings.TrimSpace(s.Body) == "" {
		p.Errors = append(p.Errors, "quotes.error.body")
	}
	if p.Errors == nil {
		// Unlike a section, a snippet has no send to stop it, so a failure is refused here before it reaches every quote.
		var err error
		if p.Failure, err = checkTypst(r.Context(), s.Body); err != nil {
			a.serverError(w, r, err)
			return s, false
		}
	}
	if p.Errors != nil || p.Failure != nil {
		a.render(w, r, http.StatusUnprocessableEntity, "snippet", p)
		return s, false
	}
	return s, true
}

func (a *App) snippetCreate(w http.ResponseWriter, r *http.Request) {
	s, ok := a.snippetForm(w, r, 0)
	if !ok {
		return
	}
	if _, err := a.db.Exec(`INSERT INTO snippet (name, language, heading, body) VALUES (?, ?, ?, ?)`, s.Name, s.Language, s.Heading, s.Body); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/snippets", http.StatusSeeOther)
}

func (a *App) snippetEdit(w http.ResponseWriter, r *http.Request) {
	if s, ok := a.snippetFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "snippet", snippetPage{Snippet: s, Languages: languageOptions()})
	}
}

func (a *App) snippetUpdate(w http.ResponseWriter, r *http.Request) {
	old, ok := a.snippetFromPath(w, r)
	if !ok {
		return
	}
	s, ok := a.snippetForm(w, r, old.ID)
	if !ok {
		return
	}
	res, err := a.db.Exec(`UPDATE snippet SET name = ?, language = ?, heading = ?, body = ? WHERE id = ?`, s.Name, s.Language, s.Heading, s.Body, s.ID)
	a.afterWrite(w, r, res, err, "/snippets")
}

// Quotes hold copies, so deleting a snippet leaves them as they are.
func (a *App) snippetDelete(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(`DELETE FROM snippet WHERE id = ?`, id)
	a.afterWrite(w, r, res, err, "/snippets")
}
