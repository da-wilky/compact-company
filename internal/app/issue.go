package app

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"mime"
	"net/http"
	"os"
	"path/filepath"
	"strconv"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/issue", a.authed(a.issueConfirm))
		mux.HandleFunc("POST /invoices/{id}/issue", a.authed(a.issueDraft))
		mux.HandleFunc("GET /invoices/{id}/pdf", a.authed(func(w http.ResponseWriter, r *http.Request) {
			a.serveStoredPDF(w, r, `SELECT number, pdf_path FROM invoice WHERE id = ? AND number IS NOT NULL`)
		}))
	})
}

// issue gives invoice id the next number of its issue year, stores the snapshot, renders the PDF
// once into the data dir with its SHA-256 and commits tx. On any failure the caller's rollback
// leaves the number unused, and no file is left behind. sql.ErrNoRows: id was already issued.
func (a *App) issue(ctx context.Context, tx *sql.Tx, id int64) error {
	issueDate, err := a.customerToday(tx, invoiceCustomer, id)
	if err != nil {
		return err
	}
	year, err := strconv.Atoi(issueDate[:4])
	if err != nil {
		return err
	}
	seq, err := nextInSequence(tx, "invoice", year)
	if err != nil {
		return err
	}
	number, err := a.cfg.Invoice.Number(year, seq)
	if err != nil {
		return err
	}
	data, gross, err := a.invoiceJSON(tx, id, &number, issueDate)
	if err != nil {
		return err
	}
	pdf, err := a.renderPDF(ctx, "invoice.typ", data)
	if err != nil {
		return err
	}
	rel := filepath.Join("invoices", strconv.Itoa(year), number+".pdf")
	return a.commitWithPDF(tx, rel, pdf, func(sha256 string) (sql.Result, error) {
		return tx.Exec(`UPDATE invoice SET number = ?1, issue_date = ?2, due_date = date(?2, '+' || payment_days || ' days'),
			service_from = ?3 ->> '$.service_period.from', service_to = ?3 ->> '$.service_period.to',
			snapshot = ?3, gross = ?4, pdf_path = ?5, pdf_sha256 = ?6,
			timezone = (SELECT c.timezone FROM customer c WHERE c.id = invoice.customer_id) WHERE id = ?7 AND number IS NULL`,
			number, issueDate, string(data), gross, rel, sha256, id)
	})
}

// nextInSequence counts up name's sequence for year; gapless because a rollback of tx takes the number back.
func nextInSequence(tx *sql.Tx, name string, year int) (seq int64, err error) {
	err = tx.QueryRow(`INSERT INTO sequence (name, year, last_value) VALUES (?, ?, 1)
		ON CONFLICT DO UPDATE SET last_value = last_value + 1 RETURNING last_value`, name, year).Scan(&seq)
	return seq, err
}

// commitWithPDF writes pdf to rel in the data dir, stores it through update, which gets its SHA-256 and
// must change exactly one row, and commits tx. On any failure no file is left behind; an update that
// changes no row is sql.ErrNoRows.
func (a *App) commitWithPDF(tx *sql.Tx, rel string, pdf []byte, update func(sha256 string) (sql.Result, error)) (err error) {
	path := filepath.Join(a.cfg.DataDir, rel)
	if err := writeNewFile(path, pdf); err != nil {
		return err
	}
	defer func() {
		if err != nil {
			_ = os.Remove(path)
		}
	}()
	sum := sha256.Sum256(pdf)
	res, err := update(hex.EncodeToString(sum[:]))
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err != nil || n != 1 {
		return errors.Join(err, sql.ErrNoRows)
	}
	return tx.Commit()
}

// writeNewFile never overwrites: an existing file is some other document's stored PDF.
func writeNewFile(path string, content []byte) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o750); err != nil {
		return err
	}
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o640)
	if err != nil {
		return err
	}
	_, err = f.Write(content)
	if err = errors.Join(err, f.Close()); err != nil {
		_ = os.Remove(path)
	}
	return err
}

func (a *App) issueConfirm(w http.ResponseWriter, r *http.Request) {
	if d, ok := a.draftFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "invoice_issue", draftPage{Draft: d})
	}
}

func (a *App) issueDraft(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	tx, err := a.db.BeginTx(r.Context(), nil)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	err = a.issue(r.Context(), tx, d.ID)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.renderDraftFailure(w, r, "invoice_issue", d, err)
		return
	}
	http.Redirect(w, r, draftPath(d.ID), http.StatusSeeOther)
}

// renderDraftFailure shows on page why d couldn't be rendered.
func (a *App) renderDraftFailure(w http.ResponseWriter, r *http.Request, page string, d draft, err error) {
	data := draftPage{Draft: d}
	if status, ok := data.explain(err); ok {
		a.render(w, r, status, page, data)
		return
	}
	a.serverError(w, r, err)
}

// renderFailure is why an invoice or quote couldn't be rendered, for the page that asked.
type renderFailure struct {
	MissingBusiness []string
	MissingEntry    string
	TypstError      string
}

// explain records err and returns the status to answer with; ok is false unless err is a render failure.
func (f *renderFailure) explain(err error) (status int, ok bool) {
	var missing *missingBusinessError
	var noEntry *missingEntryError
	var failed *typstError
	switch {
	case errors.As(err, &missing):
		f.MissingBusiness = missing.fields
		return http.StatusConflict, true
	case errors.As(err, &noEntry):
		f.MissingEntry = noEntry.entry
		return http.StatusInternalServerError, true
	case errors.As(err, &failed):
		f.TypstError = failed.message
		return http.StatusInternalServerError, true
	}
	return 0, false
}

type issuedInvoice struct {
	ID          int64
	Number      string
	Status      string
	PaidDate    string
	SHA256      string
	CancelsID   int64
	Quote       *quoteRef
	Cancellable bool
	Snapshot    invoiceData
	Lines       []issuedLine
	// AmountDue is the gross less every cancellation's; empty while no line is cancelled.
	AmountDue string
	PaidForm  string
	Errors    []string
}

type issuedLine struct {
	lineData
	ID          int64
	CancelledBy *invoiceRef
}

func (l issuedLine) project() string {
	if l.Project == nil {
		return ""
	}
	return *l.Project
}

func (inv issuedInvoice) Sections() []lineSection[issuedLine] {
	return sections(inv.Lines, issuedLine.project)
}

// invoiceStatus is the status of invoice i; cancellation is derived from its lines, never stored.
const invoiceStatus = `CASE WHEN i.number IS NULL THEN 'draft' WHEN i.kind = '` + kindCancellation + `' THEN 'cancellation'
	ELSE (SELECT CASE WHEN count(x.id) = 0 THEN iif(i.paid_date IS NULL, 'issued', 'paid')
		WHEN count(x.id) = count(*) THEN 'cancelled' ELSE 'partially_cancelled' END
		FROM invoice_line l LEFT JOIN invoice_line x ON x.cancels_line_id = l.id WHERE l.invoice_id = i.id) END`

// loadIssued returns sql.ErrNoRows unless id is issued. It reads the snapshot,
// so later changes to business or customer data don't show.
func (a *App) loadIssued(id int64) (issuedInvoice, error) {
	inv := issuedInvoice{ID: id}
	var snapshot string
	var due int64
	var quoteID sql.NullInt64
	var quoteNumber sql.NullString
	err := a.db.QueryRow(`SELECT i.number, i.snapshot, i.pdf_sha256, ifnull(i.paid_date, ''), ifnull(i.cancels_id, 0), `+invoiceStatus+`,
		`+amountDue+`, i.quote_id, q.number
		FROM invoice i LEFT JOIN quote q ON q.id = i.quote_id WHERE i.id = ? AND i.number IS NOT NULL`, id).
		Scan(&inv.Number, &snapshot, &inv.SHA256, &inv.PaidDate, &inv.CancelsID, &inv.Status, &due, &quoteID, &quoteNumber)
	if err != nil {
		return inv, err
	}
	inv.Quote = quoteRefOrNil(quoteID, quoteNumber)
	if err := json.Unmarshal([]byte(snapshot), &inv.Snapshot); err != nil {
		return inv, err
	}
	if err := a.loadIssuedLines(&inv); err != nil {
		return inv, err
	}
	inv.Cancellable = inv.Snapshot.Kind == kindInvoice && inv.Status != "cancelled"
	if inv.Status == "cancelled" || inv.Status == "partially_cancelled" {
		inv.AmountDue = formatAmount(due, currency.MustParseISO(inv.Snapshot.Currency))
	}
	return inv, nil
}

// loadIssuedLines pairs the snapshot's lines, which are in section order, with their ids and cancellations.
func (a *App) loadIssuedLines(inv *issuedInvoice) error {
	rows, err := a.db.Query(`SELECT l.id, c.id, c.number FROM invoice_line l
		LEFT JOIN invoice_line x ON x.cancels_line_id = l.id LEFT JOIN invoice c ON c.id = x.invoice_id
		WHERE l.invoice_id = ?`+sectionOrder, inv.ID)
	if err != nil {
		return err
	}
	defer func() { _ = rows.Close() }()
	for i := 0; rows.Next(); i++ {
		if i >= len(inv.Snapshot.Lines) {
			return fmt.Errorf("invoice %d has more lines than its snapshot", inv.ID)
		}
		l := issuedLine{lineData: inv.Snapshot.Lines[i]}
		var cancelID sql.NullInt64
		var cancelNumber sql.NullString
		if err := rows.Scan(&l.ID, &cancelID, &cancelNumber); err != nil {
			return err
		}
		l.CancelledBy = invoiceRefOrNil(cancelID, cancelNumber)
		inv.Lines = append(inv.Lines, l)
	}
	return rows.Err()
}

func (a *App) invoiceShow(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	d, err := a.loadDraft(id)
	if err == nil {
		a.render(w, r, http.StatusOK, "invoice", draftPage{Draft: d})
		return
	}
	if !errors.Is(err, sql.ErrNoRows) {
		a.serverError(w, r, err)
		return
	}
	inv, err := a.loadIssued(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err == nil {
		inv.PaidForm, err = a.customerToday(a.db, invoiceCustomer, id)
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "invoice_issued", inv)
}

// serveStoredPDF serves the PDF stored at issuing or sending, never a fresh rendering.
// query selects number and pdf_path of the path's id; no row is a 404.
func (a *App) serveStoredPDF(w http.ResponseWriter, r *http.Request, query string) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	var number, rel string
	err = a.db.QueryRow(query, id).Scan(&number, &rel)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	f, err := os.Open(filepath.Join(a.cfg.DataDir, rel))
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = f.Close() }()
	info, err := f.Stat()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	w.Header().Set("Content-Disposition", mime.FormatMediaType("attachment", map[string]string{"filename": number + ".pdf"}))
	http.ServeContent(w, r, number+".pdf", info.ModTime(), f)
}
