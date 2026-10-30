package app

import (
	"database/sql"
	"errors"
	"net/http"
	"slices"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /quotes/{id}/lines/new", a.authed(a.quoteLineNew))
		mux.HandleFunc("POST /quotes/{id}/lines", a.authed(a.quoteLineCreate))
		mux.HandleFunc("GET /quotes/{id}/lines/{line}", a.authed(a.quoteLineEdit))
		mux.HandleFunc("POST /quotes/{id}/lines/{line}", a.authed(a.quoteLineUpdate))
		mux.HandleFunc("POST /quotes/{id}/lines/{line}/up", a.authed(a.quotePartMove("quote_line", "line", -1)))
		mux.HandleFunc("POST /quotes/{id}/lines/{line}/down", a.authed(a.quotePartMove("quote_line", "line", +1)))
		mux.HandleFunc("POST /quotes/{id}/lines/{line}/delete", a.authed(a.quotePartDelete("quote_line", "line")))
	})
}

var intervals = []string{"once", "monthly", "yearly"}

// quoteLine holds quantity and money as the decimals the user sees.
type quoteLine struct {
	ID          int64
	Description string
	Detail      string
	Quantity    string
	Unit        string
	UnitPrice   string
	Interval    string
	Total       string
}

// intervalTotals are the totals of one interval's lines, formatted.
type intervalTotals struct {
	Interval        string
	Net, VAT, Gross string
}

// queryQuoteLines returns quote id's lines in order and each interval's line totals in minor units.
func queryQuoteLines(q querier, id int64, unit currency.Unit) ([]quoteLine, map[string][]int64, error) {
	rows, err := q.Query(`SELECT id, description, detail, quantity, unit, unit_price, interval
		FROM quote_line WHERE quote_id = ? ORDER BY position, id`, id)
	if err != nil {
		return nil, nil, err
	}
	defer func() { _ = rows.Close() }()
	var lines []quoteLine
	totals := map[string][]int64{}
	for rows.Next() {
		var l quoteLine
		var quantity, unitPrice int64
		if err := rows.Scan(&l.ID, &l.Description, &l.Detail, &quantity, &l.Unit, &unitPrice, &l.Interval); err != nil {
			return nil, nil, err
		}
		total := lineTotal(quantity, unitPrice)
		l.Quantity, l.UnitPrice, l.Total = formatQuantity(quantity), formatAmount(unitPrice, unit), formatAmount(total, unit)
		lines = append(lines, l)
		totals[l.Interval] = append(totals[l.Interval], total)
	}
	return lines, totals, rows.Err()
}

// intervalTotalsOf taxes each interval on its own net, like an invoice, and leaves out intervals without lines.
func intervalTotalsOf(totals map[string][]int64, vatPercent int64, unit currency.Unit) []intervalTotals {
	var out []intervalTotals
	for _, iv := range intervals {
		if len(totals[iv]) == 0 {
			continue
		}
		t := totalsOf(totals[iv], vatPercent)
		out = append(out, intervalTotals{iv, formatAmount(t.Net, unit), formatAmount(t.VAT, unit), formatAmount(t.Gross, unit)})
	}
	return out
}

type quoteLinePage struct {
	Quote     quoteDraft
	Line      quoteLine
	Intervals []string
	Errors    []string
}

func (a *App) renderQuoteLine(w http.ResponseWriter, r *http.Request, status int, p quoteLinePage) {
	p.Intervals = intervals
	a.render(w, r, status, "quote_line", p)
}

func (a *App) quoteLineNew(w http.ResponseWriter, r *http.Request) {
	if q, ok := a.quoteFromPath(w, r); ok {
		a.renderQuoteLine(w, r, http.StatusOK, quoteLinePage{Quote: q, Line: quoteLine{Interval: "once"}})
	}
}

// quoteLineFromPath writes 404 or 500 itself when ok is false.
func (a *App) quoteLineFromPath(w http.ResponseWriter, r *http.Request) (quoteDraft, quoteLine, bool) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return q, quoteLine{}, false
	}
	id, err := strconv.ParseInt(r.PathValue("line"), 10, 64)
	if i := slices.IndexFunc(q.Lines, func(l quoteLine) bool { return l.ID == id }); err == nil && i >= 0 {
		return q, q.Lines[i], true
	}
	a.notFound(w, r)
	return q, quoteLine{}, false
}

func (a *App) quoteLineEdit(w http.ResponseWriter, r *http.Request) {
	if q, l, ok := a.quoteLineFromPath(w, r); ok {
		a.renderQuoteLine(w, r, http.StatusOK, quoteLinePage{Quote: q, Line: l})
	}
}

// quoteLineForm reads the submitted line on q; it writes 422 itself when ok is false.
func (a *App) quoteLineForm(w http.ResponseWriter, r *http.Request, q quoteDraft, id int64) (l quoteLine, quantity, unitPrice int64, ok bool) {
	l = quoteLine{
		ID:          id,
		Description: strings.TrimSpace(r.PostFormValue("description")),
		Detail:      strings.TrimSpace(r.PostFormValue("detail")),
		Quantity:    strings.TrimSpace(r.PostFormValue("quantity")),
		Unit:        strings.TrimSpace(r.PostFormValue("unit")),
		UnitPrice:   strings.TrimSpace(r.PostFormValue("unit_price")),
		Interval:    r.PostFormValue("interval"),
	}
	var errs []string
	if l.Description == "" {
		errs = append(errs, "invoices.error.description")
	}
	quantity, err := parseQuantity(l.Quantity)
	if err != nil {
		errs = append(errs, "invoices.error.quantity")
	}
	unitPrice, err = parseAmount(l.UnitPrice, q.unit)
	if err != nil {
		errs = append(errs, "invoices.error.unit_price")
	}
	if !slices.Contains(intervals, l.Interval) {
		errs = append(errs, "quotes.error.interval")
	}
	if errs != nil {
		a.renderQuoteLine(w, r, http.StatusUnprocessableEntity, quoteLinePage{Quote: q, Line: l, Errors: errs})
		return l, 0, 0, false
	}
	return l, quantity, unitPrice, true
}

func (a *App) quoteLineCreate(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, ok := a.quoteLineForm(w, r, q, 0)
	if !ok {
		return
	}
	res, err := a.db.Exec(`INSERT INTO quote_line (quote_id, position, description, detail, quantity, unit, unit_price, interval)
		SELECT id, (SELECT ifnull(max(position), 0) + 1 FROM quote_line WHERE quote_id = ?1), ?2, ?3, ?4, ?5, ?6, ?7
		FROM quote WHERE id = ?1 AND number IS NULL`,
		q.ID, l.Description, l.Detail, quantity, l.Unit, unitPrice, l.Interval)
	a.afterInvoiceWrite(w, r, res, err, quotePath(q.ID))
}

func (a *App) quoteLineUpdate(w http.ResponseWriter, r *http.Request) {
	q, old, ok := a.quoteLineFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, ok := a.quoteLineForm(w, r, q, old.ID)
	if !ok {
		return
	}
	res, err := a.db.Exec(`UPDATE quote_line SET description = ?, detail = ?, quantity = ?, unit = ?, unit_price = ?, interval = ?
		WHERE id = ? AND quote_id IN (SELECT id FROM quote WHERE id = ? AND number IS NULL)`,
		l.Description, l.Detail, quantity, l.Unit, unitPrice, l.Interval, l.ID, q.ID)
	a.afterInvoiceWrite(w, r, res, err, quotePath(q.ID))
}

// quotePartDelete deletes the line or section (table quote_line or quote_section) named by path value param.
func (a *App) quotePartDelete(table, param string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		quoteID, errQ := strconv.ParseInt(r.PathValue("id"), 10, 64)
		id, errP := strconv.ParseInt(r.PathValue(param), 10, 64)
		if errQ != nil || errP != nil {
			a.notFound(w, r)
			return
		}
		res, err := a.db.Exec(`DELETE FROM `+table+` WHERE id = ? AND quote_id IN (SELECT id FROM quote WHERE id = ? AND number IS NULL)`, id, quoteID)
		a.afterInvoiceWrite(w, r, res, err, quotePath(quoteID))
	}
}

// quotePartMove swaps a line or section with its neighbour in direction step (-1 up, +1 down); at the edge it does nothing.
func (a *App) quotePartMove(table, param string, step int) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		quoteID, errQ := strconv.ParseInt(r.PathValue("id"), 10, 64)
		id, errP := strconv.ParseInt(r.PathValue(param), 10, 64)
		if errQ != nil || errP != nil {
			a.notFound(w, r)
			return
		}
		if err := a.swapQuoteParts(table, quoteID, id, step); errors.Is(err, sql.ErrNoRows) {
			a.notFound(w, r)
			return
		} else if err != nil {
			a.serverError(w, r, err)
			return
		}
		http.Redirect(w, r, quotePath(quoteID), http.StatusSeeOther)
	}
}

func (a *App) swapQuoteParts(table string, quoteID, id int64, step int) error {
	tx, err := a.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	var pos int64
	if err := tx.QueryRow(`SELECT l.position FROM `+table+` l JOIN quote q ON q.id = l.quote_id
		WHERE l.id = ? AND q.id = ? AND q.number IS NULL`, id, quoteID).Scan(&pos); err != nil {
		return err
	}
	neighbour := `SELECT id, position FROM ` + table + ` WHERE quote_id = ? AND position > ? ORDER BY position LIMIT 1`
	if step < 0 {
		neighbour = `SELECT id, position FROM ` + table + ` WHERE quote_id = ? AND position < ? ORDER BY position DESC LIMIT 1`
	}
	var otherID, otherPos int64
	err = tx.QueryRow(neighbour, quoteID, pos).Scan(&otherID, &otherPos)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE `+table+` SET position = ? WHERE id = ?`, otherPos, id); err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE `+table+` SET position = ? WHERE id = ?`, pos, otherID); err != nil {
		return err
	}
	return tx.Commit()
}
