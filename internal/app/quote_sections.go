package app

import (
	"context"
	"net/http"
	"slices"
	"strconv"
	"strings"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /quotes/{id}/sections/new", a.authed(a.quoteSectionNew))
		mux.HandleFunc("POST /quotes/{id}/sections", a.authed(a.quoteSectionCreate))
		mux.HandleFunc("GET /quotes/{id}/sections/{section}", a.authed(a.quoteSectionEdit))
		mux.HandleFunc("POST /quotes/{id}/sections/{section}", a.authed(a.quoteSectionUpdate))
		mux.HandleFunc("POST /quotes/{id}/sections/{section}/up", a.authed(a.quotePartMove("quote_section", "section", -1)))
		mux.HandleFunc("POST /quotes/{id}/sections/{section}/down", a.authed(a.quotePartMove("quote_section", "section", +1)))
		mux.HandleFunc("POST /quotes/{id}/sections/{section}/delete", a.authed(a.quotePartDelete("quote_section", "section")))
	})
}

// quoteSection is a heading, which may be empty, over a body of Typst markup.
// Failure is set by checkSections.
type quoteSection struct {
	ID            int64
	Heading, Body string
	Failure       *typstFailure
}

// checkSections compiles each section's body and records why it fails.
func checkSections(ctx context.Context, sections []quoteSection) error {
	for i := range sections {
		f, err := checkTypst(ctx, sections[i].Body)
		if err != nil {
			return err
		}
		sections[i].Failure = f
	}
	return nil
}

func quoteSections(q querier, quoteID int64) ([]quoteSection, error) {
	rows, err := q.Query(`SELECT id, heading, body FROM quote_section WHERE quote_id = ? ORDER BY position, id`, quoteID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var sections []quoteSection
	for rows.Next() {
		var s quoteSection
		if err := rows.Scan(&s.ID, &s.Heading, &s.Body); err != nil {
			return nil, err
		}
		sections = append(sections, s)
	}
	return sections, rows.Err()
}

type quoteSectionPage struct {
	Quote   quoteDraft
	Section quoteSection
	Errors  []string
}

func (p quoteSectionPage) Typst() typstBody { return newTypstBody(p.Section.Body) }

func (a *App) quoteSectionNew(w http.ResponseWriter, r *http.Request) {
	if q, ok := a.quoteFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "quote_section", quoteSectionPage{Quote: q})
	}
}

// quoteSectionFromPath writes 404 or 500 itself when ok is false.
func (a *App) quoteSectionFromPath(w http.ResponseWriter, r *http.Request) (quoteDraft, quoteSection, bool) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return q, quoteSection{}, false
	}
	id, err := strconv.ParseInt(r.PathValue("section"), 10, 64)
	if i := slices.IndexFunc(q.Sections, func(s quoteSection) bool { return s.ID == id }); err == nil && i >= 0 {
		return q, q.Sections[i], true
	}
	a.notFound(w, r)
	return q, quoteSection{}, false
}

func (a *App) quoteSectionEdit(w http.ResponseWriter, r *http.Request) {
	q, s, ok := a.quoteSectionFromPath(w, r)
	if !ok {
		return
	}
	var err error
	if s.Failure, err = checkTypst(r.Context(), s.Body); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "quote_section", quoteSectionPage{Quote: q, Section: s})
}

// quoteSectionForm keeps the body as typed: Typst gives whitespace meaning. It writes 422 itself when ok is false.
func (a *App) quoteSectionForm(w http.ResponseWriter, r *http.Request, q quoteDraft, id int64) (quoteSection, bool) {
	s := quoteSection{ID: id, Heading: strings.TrimSpace(r.PostFormValue("heading")), Body: r.PostFormValue("body")}
	if strings.TrimSpace(s.Body) == "" {
		a.render(w, r, http.StatusUnprocessableEntity, "quote_section", quoteSectionPage{Quote: q, Section: s, Errors: []string{"quotes.error.body"}})
		return s, false
	}
	return s, true
}

func (a *App) quoteSectionCreate(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	s, ok := a.quoteSectionForm(w, r, q, 0)
	if !ok {
		return
	}
	res, err := a.db.Exec(`INSERT INTO quote_section (quote_id, position, heading, body)
		SELECT id, (SELECT ifnull(max(position), 0) + 1 FROM quote_section WHERE quote_id = ?1), ?2, ?3
		FROM quote WHERE id = ?1 AND number IS NULL`, q.ID, s.Heading, s.Body)
	a.afterInvoiceWrite(w, r, res, err, quotePath(q.ID))
}

func (a *App) quoteSectionUpdate(w http.ResponseWriter, r *http.Request) {
	q, old, ok := a.quoteSectionFromPath(w, r)
	if !ok {
		return
	}
	s, ok := a.quoteSectionForm(w, r, q, old.ID)
	if !ok {
		return
	}
	res, err := a.db.Exec(`UPDATE quote_section SET heading = ?, body = ?
		WHERE id = ? AND quote_id IN (SELECT id FROM quote WHERE id = ? AND number IS NULL)`, s.Heading, s.Body, s.ID, q.ID)
	a.afterInvoiceWrite(w, r, res, err, quotePath(q.ID))
}
