package app

import (
	"net/http"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/preview", a.authed(a.draftPreview))
		mux.HandleFunc("GET /quotes/{id}/preview", a.authed(a.quotePreview))
	})
}

// draftPreview renders the draft without storing anything.
func (a *App) draftPreview(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	issueDate, err := a.customerToday(a.db, invoiceCustomer, d.ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	data, _, err := a.invoiceJSON(a.db, d.ID, nil, issueDate)
	if err != nil {
		a.renderDraftFailure(w, r, "invoice", d, err)
		return
	}
	pdf, err := a.renderPDF(r.Context(), "invoice.typ", data)
	if err != nil {
		a.renderDraftFailure(w, r, "invoice", d, err)
		return
	}
	writePDF(w, "draft-"+strconv.FormatInt(d.ID, 10)+".pdf", pdf)
}

// quotePreview renders the draft quote without storing anything.
func (a *App) quotePreview(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	today, err := a.customerToday(a.db, quoteCustomer, q.ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	data, err := a.quoteJSON(a.db, q.ID, nil, today)
	if err != nil {
		a.renderQuoteFailure(w, r, q, err)
		return
	}
	pdf, err := a.renderPDF(r.Context(), "quote.typ", data)
	if err != nil {
		a.renderQuoteFailure(w, r, q, err)
		return
	}
	writePDF(w, "quote-draft-"+strconv.FormatInt(q.ID, 10)+".pdf", pdf)
}

func writePDF(w http.ResponseWriter, filename string, pdf []byte) {
	w.Header().Set("Content-Type", "application/pdf")
	w.Header().Set("Content-Disposition", `inline; filename="`+filename+`"`)
	_, _ = w.Write(pdf)
}
