package app

import (
	"net/http"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/preview", a.authed(a.draftPreview))
	})
}

// draftPreview renders the draft without storing anything.
func (a *App) draftPreview(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	issueDate, err := a.issueDate(a.db, d.ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	data, err := a.invoiceJSON(a.db, d.ID, nil, issueDate)
	if err != nil {
		a.renderDraftFailure(w, r, d, err)
		return
	}
	pdf, err := a.renderPDF(r.Context(), data)
	if err != nil {
		a.renderDraftFailure(w, r, d, err)
		return
	}
	w.Header().Set("Content-Type", "application/pdf")
	w.Header().Set("Content-Disposition", `inline; filename="draft-`+strconv.FormatInt(d.ID, 10)+`.pdf"`)
	_, _ = w.Write(pdf)
}
