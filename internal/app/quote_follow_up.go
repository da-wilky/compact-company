package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /quotes/{id}/invoice", a.authed(a.quoteInvoice))
		mux.HandleFunc("POST /quotes/{id}/project", a.authed(a.quoteProject))
	})
}

// quotedHourlyRate is the hourly rate of quote q: the one unit price of its lines in hours ("h", as on
// generated invoice lines) if it is by effort, else NULL for the customer's rate.
const quotedHourlyRate = `CASE WHEN q.pricing = 'effort' THEN
	(SELECT iif(count(DISTINCT unit_price) = 1, max(unit_price), NULL) FROM quote_line WHERE quote_id = q.id AND unit = 'h') END`

// quoteProject creates a project named after an accepted quote without one and links the two both ways.
func (a *App) quoteProject(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var projectID int64
	err = tx.QueryRow(`INSERT INTO project (customer_id, name, hourly_rate, quote_id)
		SELECT q.customer_id, q.subject, `+quotedHourlyRate+`, q.id FROM quote q
		WHERE q.id = ? AND q.accepted_date IS NOT NULL AND q.project_id IS NULL RETURNING id`, id).Scan(&projectID)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err == nil {
		_, err = tx.Exec(`UPDATE quote SET project_id = ? WHERE id = ?`, projectID, id)
	}
	if err == nil {
		err = tx.Commit()
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/projects/"+strconv.FormatInt(projectID, 10), http.StatusSeeOther)
}

// quoteRef links a sent quote by its number.
type quoteRef struct {
	ID     int64
	Number string
}

func quoteRefOrNil(id sql.NullInt64, number sql.NullString) *quoteRef {
	if !id.Valid {
		return nil
	}
	return &quoteRef{ID: id.Int64, Number: number.String}
}

type projectRef struct {
	ID   int64
	Name string
}

// quoteInvoices are the invoices created from quote id, issued first, oldest first.
func (a *App) quoteInvoices(id int64) ([]invoiceRef, error) {
	rows, err := a.db.Query(`SELECT id, ifnull(number, '') FROM invoice WHERE quote_id = ? ORDER BY number IS NULL, issue_date, id`, id)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var refs []invoiceRef
	for rows.Next() {
		var ref invoiceRef
		if err := rows.Scan(&ref.ID, &ref.Number); err != nil {
			return nil, err
		}
		refs = append(refs, ref)
	}
	return refs, rows.Err()
}

// quoteInvoice opens an invoice draft for an accepted quote's customer and project, copying
// the one-off lines of a fixed-price quote as free lines; a by-effort quote is billed by time.
func (a *App) quoteInvoice(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var invoiceID int64
	err = tx.QueryRow(`INSERT INTO invoice
		(kind, customer_id, range_from, range_to, service_from, service_to, grouping, rounding_minutes, payment_days, currency, all_projects, quote_id)
		SELECT ?, customer_id, '', '', '', '', 'project', ?, ?, currency, project_id IS NULL, id FROM quote
		WHERE id = ? AND accepted_date IS NOT NULL RETURNING id`,
		kindInvoice, a.cfg.Invoice.RoundingMinutes, a.cfg.Invoice.PaymentDays, id).Scan(&invoiceID)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err == nil {
		_, err = tx.Exec(`INSERT INTO invoice_project (invoice_id, project_id)
			SELECT ?, project_id FROM quote WHERE id = ? AND project_id IS NOT NULL`, invoiceID, id)
	}
	if err == nil {
		_, err = tx.Exec(`INSERT INTO invoice_line (invoice_id, position, generated, description, detail, quantity, unit, unit_price)
			SELECT ?, l.position, 0, l.description, l.detail, l.quantity, l.unit, l.unit_price
			FROM quote_line l JOIN quote q ON q.id = l.quote_id
			WHERE q.id = ? AND q.pricing = 'fixed' AND l.interval = 'once'`, invoiceID, id)
	}
	if err == nil {
		err = tx.Commit()
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, draftPath(invoiceID), http.StatusSeeOther)
}
