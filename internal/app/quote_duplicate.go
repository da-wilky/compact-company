package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /quotes/{id}/duplicate", a.authed(a.quoteDuplicateForm))
		mux.HandleFunc("POST /quotes/{id}/duplicate", a.authed(a.quoteDuplicate))
	})
}

// quoteSource is any quote, draft or sent, as duplicating needs it.
type quoteSource struct {
	ID         int64
	CustomerID int64
	Subject    string
	currency   string
}

type quoteDuplicatePage struct {
	Quote     quoteSource
	Customer  string
	Customers []customerOption
	Errors    []string
}

func (a *App) quoteSourceFromPath(w http.ResponseWriter, r *http.Request) (quoteSource, bool) {
	var q quoteSource
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err == nil {
		q.ID = id
		err = a.db.QueryRow(`SELECT customer_id, subject, currency FROM quote WHERE id = ?`, id).Scan(&q.CustomerID, &q.Subject, &q.currency)
	}
	if err == nil {
		return q, true
	}
	var numErr *strconv.NumError
	if errors.Is(err, sql.ErrNoRows) || errors.As(err, &numErr) {
		a.notFound(w, r)
	} else {
		a.serverError(w, r, err)
	}
	return q, false
}

func (a *App) renderQuoteDuplicate(w http.ResponseWriter, r *http.Request, status int, p quoteDuplicatePage) {
	var err error
	if p.Customers, err = a.customerOptions(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "quote_duplicate", p)
}

func (a *App) quoteDuplicateForm(w http.ResponseWriter, r *http.Request) {
	if q, ok := a.quoteSourceFromPath(w, r); ok {
		a.renderQuoteDuplicate(w, r, http.StatusOK, quoteDuplicatePage{Quote: q, Customer: strconv.FormatInt(q.CustomerID, 10)})
	}
}

// quoteDuplicate copies subject, pricing, lines and sections into a new draft; language, currency and validity
// start afresh from the chosen customer and today, and the project stays only for the same customer.
func (a *App) quoteDuplicate(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteSourceFromPath(w, r)
	if !ok {
		return
	}
	customer, _ := strconv.ParseInt(r.PostFormValue("customer"), 10, 64)
	var language, cur string
	err := a.db.QueryRow(`SELECT invoice_language, currency FROM customer WHERE id = ?`, customer).Scan(&language, &cur)
	if errors.Is(err, sql.ErrNoRows) {
		a.renderQuoteDuplicate(w, r, http.StatusUnprocessableEntity, quoteDuplicatePage{Quote: q, Customer: r.PostFormValue("customer"), Errors: []string{"invoices.error.customer"}})
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	validUntil, err := a.newQuoteValidUntil(customer)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	id, err := duplicateQuote(tx, q, customer, language, cur, validUntil)
	if err == nil {
		err = tx.Commit()
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, quotePath(id), http.StatusSeeOther)
}

func duplicateQuote(tx *sql.Tx, q quoteSource, customer int64, language, cur, validUntil string) (int64, error) {
	var id int64
	err := tx.QueryRow(`INSERT INTO quote (customer_id, project_id, subject, language, currency, pricing, valid_until)
		SELECT ?1, CASE WHEN customer_id = ?1 THEN project_id END, subject, ?2, ?3, pricing, ?4 FROM quote WHERE id = ?5 RETURNING id`,
		customer, language, cur, validUntil, q.ID).Scan(&id)
	if err != nil {
		return 0, err
	}
	if _, err := tx.Exec(`INSERT INTO quote_section (quote_id, position, heading, body)
		SELECT ?, position, heading, body FROM quote_section WHERE quote_id = ?`, id, q.ID); err != nil {
		return 0, err
	}
	if _, err := tx.Exec(`INSERT INTO quote_line (quote_id, position, description, detail, quantity, unit, unit_price, interval)
		SELECT ?, position, description, detail, quantity, unit, unit_price, interval FROM quote_line WHERE quote_id = ?`, id, q.ID); err != nil {
		return 0, err
	}
	if cur == q.currency {
		return id, nil
	}
	return id, rescaleQuotePrices(tx, id, currency.MustParseISO(q.currency), currency.MustParseISO(cur))
}

func rescaleQuotePrices(tx *sql.Tx, quoteID int64, from, to currency.Unit) error {
	rows, err := tx.Query(`SELECT id, unit_price FROM quote_line WHERE quote_id = ?`, quoteID)
	if err != nil {
		return err
	}
	prices := map[int64]int64{}
	for rows.Next() {
		var id, price int64
		if err := rows.Scan(&id, &price); err != nil {
			_ = rows.Close()
			return err
		}
		prices[id] = price
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	for id, price := range prices {
		if _, err := tx.Exec(`UPDATE quote_line SET unit_price = ? WHERE id = ?`, rescaleAmount(price, from, to), id); err != nil {
			return err
		}
	}
	return nil
}
