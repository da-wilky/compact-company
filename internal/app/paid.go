package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /invoices/{id}/paid", a.authed(a.markPaid))
		mux.HandleFunc("POST /invoices/{id}/unpaid", a.authed(a.markUnpaid))
	})
}

func (a *App) markPaid(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	date := r.FormValue("paid_date")
	if _, err := time.Parse(time.DateOnly, date); err != nil {
		a.refusePaid(w, r, id, date, http.StatusUnprocessableEntity, "invoices.error.paid_date")
		return
	}
	var cancelled bool
	if err := a.db.QueryRow(`SELECT EXISTS (SELECT 1 FROM invoice_line WHERE invoice_id = ?1)
		AND NOT EXISTS (SELECT 1 FROM invoice_line l WHERE l.invoice_id = ?1 AND NOT EXISTS (
			SELECT 1 FROM invoice_line cl JOIN invoice ci ON ci.id = cl.invoice_id
			WHERE cl.cancels_line_id = l.id AND ci.number IS NOT NULL))`, id).Scan(&cancelled); err != nil {
		a.serverError(w, r, err)
		return
	}
	if cancelled {
		a.refusePaid(w, r, id, date, http.StatusConflict, "invoices.error.paid_cancelled")
		return
	}
	res, err := a.db.Exec(`UPDATE invoice SET paid_date = ? WHERE id = ? AND kind = ? AND number IS NOT NULL`, date, id, kindInvoice)
	a.afterInvoiceWrite(w, r, res, err, draftPath(id))
}

func (a *App) markUnpaid(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(`UPDATE invoice SET paid_date = NULL WHERE id = ? AND number IS NOT NULL`, id)
	a.afterInvoiceWrite(w, r, res, err, draftPath(id))
}

// refusePaid shows issued invoice id with the submitted date and why it wasn't marked paid.
func (a *App) refusePaid(w http.ResponseWriter, r *http.Request, id int64, date string, status int, reason string) {
	inv, err := a.loadIssued(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	inv.PaidForm, inv.Errors = date, []string{reason}
	a.render(w, r, status, "invoice_issued", inv)
}
