package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/lines/new", a.authed(a.lineNew))
		mux.HandleFunc("POST /invoices/{id}/lines", a.authed(a.lineCreate))
		mux.HandleFunc("GET /invoices/{id}/lines/{line}", a.authed(a.lineEdit))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}", a.authed(a.lineUpdate))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/up", a.authed(a.lineMove(-1)))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/down", a.authed(a.lineMove(+1)))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/delete", a.authed(a.lineDelete))
	})
}

// invoiceLine holds quantity and money as the decimals the user sees.
type invoiceLine struct {
	ID          int64
	Generated   bool
	Description string
	Detail      string
	Quantity    string
	Unit        string
	UnitPrice   string
	Total       string
}

// loadLines returns invoice id's lines in position order and their totals at vatPercent.
func (a *App) loadLines(id int64, unit currency.Unit, vatPercent int64) ([]invoiceLine, invoiceTotals, error) {
	lines, totals, err := queryLines(a.db, id, unit)
	return lines, totalsOf(totals, vatPercent), err
}

// queryLines returns invoice id's lines in position order and each line's total in minor units.
func queryLines(q querier, id int64, unit currency.Unit) ([]invoiceLine, []int64, error) {
	rows, err := q.Query(`SELECT id, generated, description, detail, quantity, unit, unit_price
		FROM invoice_line WHERE invoice_id = ? ORDER BY position, id`, id)
	if err != nil {
		return nil, nil, err
	}
	defer func() { _ = rows.Close() }()
	var lines []invoiceLine
	var totals []int64
	for rows.Next() {
		var l invoiceLine
		var quantity, unitPrice int64
		if err := rows.Scan(&l.ID, &l.Generated, &l.Description, &l.Detail, &quantity, &l.Unit, &unitPrice); err != nil {
			return nil, nil, err
		}
		total := lineTotal(quantity, unitPrice)
		l.Quantity, l.UnitPrice, l.Total = formatQuantity(quantity), formatAmount(unitPrice, unit), formatAmount(total, unit)
		lines = append(lines, l)
		totals = append(totals, total)
	}
	return lines, totals, rows.Err()
}

type linePage struct {
	Draft   draft
	Line    invoiceLine
	Entries []lineEntry
	Errors  []string
}

// renderLine shows an existing line together with the entries it claims.
func (a *App) renderLine(w http.ResponseWriter, r *http.Request, status int, p linePage) {
	entries, err := a.lineEntries(a.db, p.Draft.ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	p.Entries = entries[p.Line.ID]
	a.render(w, r, status, "invoice_line", p)
}

func (a *App) lineNew(w http.ResponseWriter, r *http.Request) {
	if d, ok := a.draftFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "invoice_line", linePage{Draft: d})
	}
}

// lineFromPath writes 404 or 500 itself when ok is false.
func (a *App) lineFromPath(w http.ResponseWriter, r *http.Request) (draft, invoiceLine, bool) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return d, invoiceLine{}, false
	}
	id, err := strconv.ParseInt(r.PathValue("line"), 10, 64)
	if err == nil {
		for _, l := range d.Lines {
			if l.ID == id {
				return d, l, true
			}
		}
	}
	a.notFound(w, r)
	return d, invoiceLine{}, false
}

func (a *App) lineEdit(w http.ResponseWriter, r *http.Request) {
	if d, l, ok := a.lineFromPath(w, r); ok {
		a.renderLine(w, r, http.StatusOK, linePage{Draft: d, Line: l})
	}
}

// lineFromForm returns the submitted line, its quantity in thousandths, its unit price
// in minor units and any validation errors as catalog keys.
func lineFromForm(r *http.Request, unit currency.Unit) (invoiceLine, int64, int64, []string) {
	l := invoiceLine{
		Description: strings.TrimSpace(r.PostFormValue("description")),
		Detail:      strings.TrimSpace(r.PostFormValue("detail")),
		Quantity:    strings.TrimSpace(r.PostFormValue("quantity")),
		Unit:        strings.TrimSpace(r.PostFormValue("unit")),
		UnitPrice:   strings.TrimSpace(r.PostFormValue("unit_price")),
	}
	var errs []string
	if l.Description == "" {
		errs = append(errs, "invoices.error.description")
	}
	quantity, err := parseQuantity(l.Quantity)
	if err != nil {
		errs = append(errs, "invoices.error.quantity")
	}
	unitPrice, err := parseAmount(l.UnitPrice, unit)
	if err != nil {
		errs = append(errs, "invoices.error.unit_price")
	}
	return l, quantity, unitPrice, errs
}

func (a *App) lineCreate(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, errs := lineFromForm(r, d.unit)
	if errs != nil {
		a.render(w, r, http.StatusUnprocessableEntity, "invoice_line", linePage{Draft: d, Line: l, Errors: errs})
		return
	}
	res, err := a.db.Exec(`INSERT INTO invoice_line (invoice_id, position, generated, description, detail, quantity, unit, unit_price)
		SELECT id, (SELECT ifnull(max(position), 0) + 1 FROM invoice_line WHERE invoice_id = ?1), 0, ?2, ?3, ?4, ?5, ?6
		FROM invoice WHERE id = ?1 AND number IS NULL`,
		d.ID, l.Description, l.Detail, quantity, l.Unit, unitPrice)
	a.afterDraftWrite(w, r, res, err, draftPath(d.ID))
}

func (a *App) lineUpdate(w http.ResponseWriter, r *http.Request) {
	d, old, ok := a.lineFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, errs := lineFromForm(r, d.unit)
	l.ID, l.Generated = old.ID, old.Generated
	if errs != nil {
		a.renderLine(w, r, http.StatusUnprocessableEntity, linePage{Draft: d, Line: l, Errors: errs})
		return
	}
	res, err := a.db.Exec(`UPDATE invoice_line SET description = ?, detail = ?, quantity = ?, unit = ?, unit_price = ?
		WHERE id = ? AND invoice_id IN (SELECT id FROM invoice WHERE id = ? AND number IS NULL)`,
		l.Description, l.Detail, quantity, l.Unit, unitPrice, l.ID, d.ID)
	a.afterDraftWrite(w, r, res, err, draftPath(d.ID))
}

// Entries claimed by the line are released by the foreign key's ON DELETE SET NULL.
func (a *App) lineDelete(w http.ResponseWriter, r *http.Request) {
	d, l, ok := a.lineFromPath(w, r)
	if !ok {
		return
	}
	res, err := a.db.Exec(`DELETE FROM invoice_line
		WHERE id = ? AND invoice_id IN (SELECT id FROM invoice WHERE id = ? AND number IS NULL)`, l.ID, d.ID)
	a.afterDraftWrite(w, r, res, err, draftPath(d.ID))
}

// lineMove swaps the line with its neighbour in direction step (-1 up, +1 down); at the edge it does nothing.
func (a *App) lineMove(step int) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		d, l, ok := a.lineFromPath(w, r)
		if !ok {
			return
		}
		if err := a.swapLines(d.ID, l.ID, step); errors.Is(err, sql.ErrNoRows) {
			a.notFound(w, r)
			return
		} else if err != nil {
			a.serverError(w, r, err)
			return
		}
		http.Redirect(w, r, draftPath(d.ID), http.StatusSeeOther)
	}
}

func (a *App) swapLines(invoiceID, lineID int64, step int) error {
	tx, err := a.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	var pos int64
	if err := tx.QueryRow(`SELECT l.position FROM invoice_line l JOIN invoice i ON i.id = l.invoice_id
		WHERE l.id = ? AND i.id = ? AND i.number IS NULL`, lineID, invoiceID).Scan(&pos); err != nil {
		return err
	}
	neighbour := `SELECT id, position FROM invoice_line WHERE invoice_id = ? AND position > ? ORDER BY position LIMIT 1`
	if step < 0 {
		neighbour = `SELECT id, position FROM invoice_line WHERE invoice_id = ? AND position < ? ORDER BY position DESC LIMIT 1`
	}
	var otherID, otherPos int64
	err = tx.QueryRow(neighbour, invoiceID, pos).Scan(&otherID, &otherPos)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE invoice_line SET position = ? WHERE id = ?`, otherPos, lineID); err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE invoice_line SET position = ? WHERE id = ?`, pos, otherID); err != nil {
		return err
	}
	return tx.Commit()
}
