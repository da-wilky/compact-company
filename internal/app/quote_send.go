package app

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"path/filepath"
	"slices"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /quotes/{id}/send", a.authed(a.quoteSendConfirm))
		mux.HandleFunc("POST /quotes/{id}/send", a.authed(a.quoteSend))
		mux.HandleFunc("GET /quotes/{id}/pdf", a.authed(func(w http.ResponseWriter, r *http.Request) {
			a.serveStoredPDF(w, r, `SELECT number, pdf_path FROM quote WHERE id = ? AND number IS NOT NULL`)
		}))
	})
}

// sendQuote gives quote id the next number of its send year, stores the snapshot, renders the PDF
// once into the data dir with its SHA-256 and commits tx, all or nothing like issuing an invoice.
// sql.ErrNoRows: id was already sent.
func (a *App) sendQuote(ctx context.Context, tx *sql.Tx, id int64) error {
	sendDate, err := a.quoteToday(tx, id)
	if err != nil {
		return err
	}
	year, err := strconv.Atoi(sendDate[:4])
	if err != nil {
		return err
	}
	seq, err := nextInSequence(tx, "quote", year)
	if err != nil {
		return err
	}
	number, err := a.cfg.Quote.Number(year, seq)
	if err != nil {
		return err
	}
	data, err := a.quoteJSON(tx, id, &number, sendDate)
	if err != nil {
		return err
	}
	pdf, err := a.renderPDF(ctx, "quote.typ", data)
	if err != nil {
		return err
	}
	rel := filepath.Join("quotes", strconv.Itoa(year), number+".pdf")
	return a.commitWithPDF(tx, rel, pdf, func(sha256 string) (sql.Result, error) {
		return tx.Exec(`UPDATE quote SET number = ?, send_date = ?, snapshot = ?, pdf_path = ?, pdf_sha256 = ?
			WHERE id = ? AND number IS NULL`, number, sendDate, string(data), rel, sha256, id)
	})
}

// sendRefusal is why q can't be sent before rendering it: a section that doesn't compile.
func sendRefusal(ctx context.Context, q quoteDraft) ([]string, error) {
	if err := checkSections(ctx, q.Sections); err != nil {
		return nil, err
	}
	if slices.ContainsFunc(q.Sections, func(s quoteSection) bool { return s.Failure != nil }) {
		return []string{"quotes.error.sections_fail"}, nil
	}
	return nil, nil
}

func (a *App) quoteSendConfirm(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	errs, err := sendRefusal(r.Context(), q)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "quote_send", quotePage{Quote: q, Errors: errs})
}

func (a *App) quoteSend(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	errs, err := sendRefusal(r.Context(), q)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if errs != nil {
		a.render(w, r, http.StatusUnprocessableEntity, "quote_send", quotePage{Quote: q, Errors: errs})
		return
	}
	tx, err := a.db.BeginTx(r.Context(), nil)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	err = a.sendQuote(r.Context(), tx, q.ID)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		p := quotePage{Quote: q}
		status, ok := p.explain(err)
		if !ok {
			a.serverError(w, r, err)
			return
		}
		a.render(w, r, status, "quote_send", p)
		return
	}
	http.Redirect(w, r, quotePath(q.ID), http.StatusSeeOther)
}

type sentQuote struct {
	ID           int64
	Number       string
	SHA256       string
	Snapshot     quoteData
	Status       string
	AcceptedDate string
	DeclinedDate string
	DateForm     string
	Errors       []string
	// Project is the quote's project now, which creating one from the accepted quote may have set since sending.
	Project  *projectRef
	Invoices []invoiceRef
}

// Totals are the snapshot's totals of the intervals that have lines.
func (q sentQuote) Totals() []intervalTotals {
	var out []intervalTotals
	for i, t := range []*totalsData{q.Snapshot.Totals.Once, q.Snapshot.Totals.Monthly, q.Snapshot.Totals.Yearly} {
		if t != nil {
			out = append(out, intervalTotals{intervals[i], t.Net, t.VAT, t.Gross})
		}
	}
	return out
}

// loadSentQuote returns sql.ErrNoRows unless id is sent. It reads the snapshot,
// so later changes to business, customer or project data don't show.
func (a *App) loadSentQuote(id int64) (sentQuote, error) {
	q := sentQuote{ID: id}
	var snapshot, validUntil string
	var projectID sql.NullInt64
	var projectName sql.NullString
	err := a.db.QueryRow(`SELECT q.number, q.snapshot, q.pdf_sha256, `+quoteStatus+`, q.valid_until,
		ifnull(q.accepted_date, ''), ifnull(q.declined_date, ''), p.id, p.name
		FROM quote q LEFT JOIN project p ON p.id = q.project_id WHERE q.id = ? AND q.number IS NOT NULL`, id).
		Scan(&q.Number, &snapshot, &q.SHA256, &q.Status, &validUntil, &q.AcceptedDate, &q.DeclinedDate, &projectID, &projectName)
	if err != nil {
		return q, err
	}
	if projectID.Valid {
		q.Project = &projectRef{projectID.Int64, projectName.String}
	}
	if q.Invoices, err = a.quoteInvoices(id); err != nil {
		return q, err
	}
	if q.DateForm, err = a.quoteToday(a.db, id); err != nil {
		return q, err
	}
	q.Status = expire(q.Status, validUntil, q.DateForm)
	return q, json.Unmarshal([]byte(snapshot), &q.Snapshot)
}
