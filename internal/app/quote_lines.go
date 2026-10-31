package app

import (
	"database/sql"
	"errors"
	"net/http"
	"slices"
	"strconv"

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

type quoteLine struct {
	ID int64
	lineFields
	Interval string
}

// intervalTotals are the totals of one interval's lines, formatted.
type intervalTotals struct {
	Interval string
	totalsData
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
	fields, quantity, unitPrice, errs := lineFieldsFromForm(r, q.unit)
	l = quoteLine{ID: id, lineFields: fields, Interval: r.PostFormValue("interval")}
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
		if err := a.swapPosition(table, "quote", quoteID, id, step, "quote_id"); errors.Is(err, sql.ErrNoRows) {
			a.notFound(w, r)
			return
		} else if err != nil {
			a.serverError(w, r, err)
			return
		}
		http.Redirect(w, r, quotePath(quoteID), http.StatusSeeOther)
	}
}
