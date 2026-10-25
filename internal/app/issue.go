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
	"strings"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/issue", a.authed(a.issueConfirm))
		mux.HandleFunc("POST /invoices/{id}/issue", a.authed(a.issueDraft))
		mux.HandleFunc("GET /invoices/{id}/pdf", a.authed(a.invoicePDF))
	})
}

func formatInvoiceNumber(format string, year int, seq int64) (string, error) {
	number, ok := substituteSeq(strings.Replace(format, "{year}", strconv.Itoa(year), 1), seq)
	if !ok || strings.Count(format, "{year}") != 1 {
		return "", fmt.Errorf("invoice.number_format: must contain {year} and {seq} or {seq:0N} exactly once each, got %q", format)
	}
	return number, nil
}

// checkInvoiceNumberFormat refuses formats whose numbers aren't safe file names under invoices/<year>/.
// Placeholders only ever yield digits, so one sample number speaks for all of them.
func checkInvoiceNumberFormat(format string) error {
	number, err := formatInvoiceNumber(format, 2026, 1)
	if err != nil {
		return err
	}
	if strings.Contains(number, "/") || strings.Contains(number, "..") {
		return fmt.Errorf(`invoice.number_format: must not yield "/" or "..", got %q`, format)
	}
	return nil
}

// issue gives invoice id the next number of its issue year, stores the snapshot, renders the PDF
// once into the data dir with its SHA-256 and commits tx. On any failure the caller's rollback
// leaves the number unused, and no file is left behind. sql.ErrNoRows: id was already issued.
func (a *App) issue(ctx context.Context, tx *sql.Tx, id int64) (err error) {
	issueDate, err := a.issueDate(tx, id)
	if err != nil {
		return err
	}
	year, err := strconv.Atoi(issueDate[:4])
	if err != nil {
		return err
	}
	var seq int64
	if err := tx.QueryRow(`INSERT INTO sequence (name, year, last_value) VALUES ('invoice', ?, 1)
		ON CONFLICT DO UPDATE SET last_value = last_value + 1 RETURNING last_value`, year).Scan(&seq); err != nil {
		return err
	}
	number, err := formatInvoiceNumber(a.cfg.Invoice.NumberFormat, year, seq)
	if err != nil {
		return err
	}
	data, err := a.invoiceJSON(tx, id, &number, issueDate)
	if err != nil {
		return err
	}
	pdf, err := a.renderPDF(ctx, data)
	if err != nil {
		return err
	}
	rel := filepath.Join("invoices", strconv.Itoa(year), number+".pdf")
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
	res, err := tx.Exec(`UPDATE invoice SET number = ?1, issue_date = ?2, due_date = date(?2, '+' || payment_days || ' days'),
		snapshot = ?3, pdf_path = ?4, pdf_sha256 = ?5 WHERE id = ?6 AND number IS NULL`,
		number, issueDate, string(data), rel, hex.EncodeToString(sum[:]), id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err != nil || n != 1 {
		return errors.Join(err, sql.ErrNoRows)
	}
	return tx.Commit()
}

// writeNewFile never overwrites: an existing file is some other invoice's stored PDF.
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
		a.renderDraftFailure(w, r, d, err)
		return
	}
	http.Redirect(w, r, draftPath(d.ID), http.StatusSeeOther)
}

// renderDraftFailure shows why d couldn't be rendered on the draft's page.
func (a *App) renderDraftFailure(w http.ResponseWriter, r *http.Request, d draft, err error) {
	var missing *missingBusinessError
	var failed *typstError
	switch {
	case errors.As(err, &missing):
		a.render(w, r, http.StatusConflict, "invoice", draftPage{Draft: d, MissingBusiness: missing.fields})
	case errors.As(err, &failed):
		a.render(w, r, http.StatusInternalServerError, "invoice", draftPage{Draft: d, TypstError: failed.message})
	default:
		a.serverError(w, r, err)
	}
}

type issuedInvoice struct {
	ID        int64
	Number    string
	Status    string
	PaidDate  string
	SHA256    string
	CancelsID int64
	Snapshot  invoiceData
	Lines     []issuedLine
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

// invoiceStatus is the status of invoice i; cancellation is derived from its lines, never stored.
const invoiceStatus = `CASE WHEN i.number IS NULL THEN 'draft' WHEN i.kind = 'cancellation' THEN 'cancellation'
	ELSE (SELECT CASE WHEN count(x.id) = 0 THEN iif(i.paid_date IS NULL, 'issued', 'paid')
		WHEN count(x.id) = count(*) THEN 'cancelled' ELSE 'partially_cancelled' END
		FROM invoice_line l LEFT JOIN invoice_line x ON x.cancels_line_id = l.id WHERE l.invoice_id = i.id) END`

// loadIssued returns sql.ErrNoRows unless id is issued. It reads the snapshot,
// so later changes to business or customer data don't show.
func (a *App) loadIssued(id int64) (issuedInvoice, error) {
	inv := issuedInvoice{ID: id}
	var snapshot string
	err := a.db.QueryRow(`SELECT i.number, i.snapshot, i.pdf_sha256, ifnull(i.paid_date, ''), ifnull(i.cancels_id, 0), `+invoiceStatus+`
		FROM invoice i WHERE i.id = ? AND i.number IS NOT NULL`, id).
		Scan(&inv.Number, &snapshot, &inv.SHA256, &inv.PaidDate, &inv.CancelsID, &inv.Status)
	if err != nil {
		return inv, err
	}
	if err := json.Unmarshal([]byte(snapshot), &inv.Snapshot); err != nil {
		return inv, err
	}
	if err := a.loadIssuedLines(&inv); err != nil {
		return inv, err
	}
	if inv.Status == "cancelled" || inv.Status == "partially_cancelled" {
		inv.AmountDue, err = a.amountDue(inv)
	}
	return inv, err
}

// loadIssuedLines pairs the snapshot's lines, which are in position order, with their ids and cancellations.
func (a *App) loadIssuedLines(inv *issuedInvoice) error {
	rows, err := a.db.Query(`SELECT l.id, c.id, c.number FROM invoice_line l
		LEFT JOIN invoice_line x ON x.cancels_line_id = l.id LEFT JOIN invoice c ON c.id = x.invoice_id
		WHERE l.invoice_id = ? ORDER BY l.position, l.id`, inv.ID)
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
		l.CancelledBy = claimedBy(cancelID, cancelNumber)
		inv.Lines = append(inv.Lines, l)
	}
	return rows.Err()
}

func (a *App) amountDue(inv issuedInvoice) (string, error) {
	unit := currency.MustParseISO(inv.Snapshot.Currency)
	due, err := parseAmount(inv.Snapshot.Totals.Gross, unit)
	if err != nil {
		return "", err
	}
	rows, err := a.db.Query(`SELECT snapshot ->> '$.totals.gross' FROM invoice WHERE cancels_id = ?`, inv.ID)
	if err != nil {
		return "", err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var gross string
		if err := rows.Scan(&gross); err != nil {
			return "", err
		}
		cancelled, err := parseAmount(strings.TrimPrefix(gross, "-"), unit)
		if err != nil {
			return "", err
		}
		due -= cancelled
	}
	return formatAmount(due, unit), rows.Err()
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
		inv.PaidForm, err = a.issueDate(a.db, id)
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "invoice_issued", inv)
}

// invoicePDF serves the PDF stored at issuing, never a fresh rendering.
func (a *App) invoicePDF(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	var number, rel string
	err = a.db.QueryRow(`SELECT number, pdf_path FROM invoice WHERE id = ? AND number IS NOT NULL`, id).Scan(&number, &rel)
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
