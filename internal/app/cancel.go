package app

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/cancel", a.authed(a.cancelConfirm))
		mux.HandleFunc("POST /invoices/{id}/cancel", a.authed(a.cancelLines))
	})
}

type cancelPage struct {
	Invoice         issuedInvoice
	Reason          string
	Errors          []string
	MissingBusiness []string
	TypstError      string
}

// cancellableFromPath loads the issued invoice in the path; cancellation invoices can't be cancelled.
// It writes 404 or 500 itself when ok is false.
func (a *App) cancellableFromPath(w http.ResponseWriter, r *http.Request) (issuedInvoice, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return issuedInvoice{}, false
	}
	inv, err := a.loadIssued(id)
	if errors.Is(err, sql.ErrNoRows) || err == nil && inv.Snapshot.Kind != kindInvoice {
		a.notFound(w, r)
		return inv, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return inv, false
	}
	return inv, true
}

func (a *App) cancelConfirm(w http.ResponseWriter, r *http.Request) {
	if inv, ok := a.cancellableFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "invoice_cancel", cancelPage{Invoice: inv})
	}
}

// cancelLines issues a cancellation invoice reversing the ticked lines and releases their entries.
func (a *App) cancelLines(w http.ResponseWriter, r *http.Request) {
	inv, ok := a.cancellableFromPath(w, r)
	if !ok {
		return
	}
	page := cancelPage{Invoice: inv, Reason: strings.TrimSpace(r.PostFormValue("reason"))}
	ticked := map[string]bool{}
	for _, v := range r.PostForm["line"] {
		ticked[v] = true
	}
	var ids []int64
	for _, l := range inv.Lines {
		if ticked[strconv.FormatInt(l.ID, 10)] && l.CancelledBy == nil {
			ids = append(ids, l.ID)
		}
	}
	if len(ids) == 0 || len(ids) != len(ticked) {
		page.Errors = []string{"invoices.error.cancel_lines"}
		a.render(w, r, http.StatusUnprocessableEntity, "invoice_cancel", page)
		return
	}
	idList, err := json.Marshal(ids)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	tx, err := a.db.BeginTx(r.Context(), nil)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var cancellation int64
	err = tx.QueryRow(`INSERT INTO invoice
		(kind, customer_id, cancels_id, service_from, service_to, rounding_minutes, payment_days, currency, reason)
		SELECT ?, customer_id, id, service_from, service_to, rounding_minutes, payment_days, currency, ?
		FROM invoice WHERE id = ? RETURNING id`, kindCancellation, page.Reason, inv.ID).Scan(&cancellation)
	if err == nil {
		_, err = tx.Exec(`INSERT INTO invoice_line
			(invoice_id, position, generated, description, detail, quantity, unit, unit_price, cancels_line_id)
			SELECT ?, position, 0, description, detail, -quantity, unit, unit_price, id
			FROM invoice_line WHERE id IN (SELECT value FROM json_each(?))`, cancellation, string(idList))
	}
	if err == nil {
		_, err = tx.Exec(`UPDATE time_entry SET invoice_line_id = NULL WHERE invoice_line_id IN (SELECT value FROM json_each(?))`, string(idList))
	}
	if err == nil {
		err = a.issue(r.Context(), tx, cancellation)
	}
	var missing *missingBusinessError
	var failed *typstError
	switch {
	case err == nil:
		http.Redirect(w, r, draftPath(cancellation), http.StatusSeeOther)
	case errors.As(err, &missing):
		page.MissingBusiness = missing.fields
		a.render(w, r, http.StatusConflict, "invoice_cancel", page)
	case errors.As(err, &failed):
		page.TypstError = failed.message
		a.render(w, r, http.StatusInternalServerError, "invoice_cancel", page)
	default:
		a.serverError(w, r, err)
	}
}
