package app

import (
	"errors"
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
	var missing *missingBusinessError
	if errors.As(err, &missing) {
		a.render(w, r, http.StatusConflict, "invoice", draftPage{Draft: d, MissingBusiness: missing.fields})
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	pdf, err := a.renderPDF(r.Context(), data)
	var failed *typstError
	if errors.As(err, &failed) {
		a.render(w, r, http.StatusInternalServerError, "invoice", draftPage{Draft: d, TypstError: failed.message})
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	w.Header().Set("Content-Type", "application/pdf")
	w.Header().Set("Content-Disposition", `inline; filename="draft-`+strconv.FormatInt(d.ID, 10)+`.pdf"`)
	_, _ = w.Write(pdf)
}
