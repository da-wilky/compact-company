package app

import (
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /customers", a.authed(a.customerList))
		mux.HandleFunc("GET /customers/new", a.authed(a.customerNew))
		mux.HandleFunc("POST /customers", a.authed(a.customerCreate))
		mux.HandleFunc("GET /customers/{id}", a.authed(a.customerEdit))
		mux.HandleFunc("POST /customers/{id}", a.authed(a.customerUpdate))
	})
}

var seqPattern = regexp.MustCompile(`\{seq(?::0(\d+))?\}`)

func formatCustomerNumber(format string, seq int64) (string, error) {
	locs := seqPattern.FindAllStringSubmatchIndex(format, -1)
	if len(locs) != 1 {
		return "", fmt.Errorf("customer_number_format: must contain {seq} or {seq:0N} exactly once, got %q", format)
	}
	m := locs[0]
	width := 0
	if m[2] >= 0 {
		width, _ = strconv.Atoi(format[m[2]:m[3]])
	}
	return format[:m[0]] + fmt.Sprintf("%0*d", width, seq) + format[m[1]:], nil
}

// customer holds the hourly rate as the decimal the user sees; the database stores minor units.
type customer struct {
	ID              int64
	Number          string
	Name            string
	Address         string
	Email           string
	Currency        string
	InvoiceLanguage string
	Timezone        string
	HourlyRate      string
	Note            string
}

type customerPage struct {
	Customer  customer
	Errors    []string
	Languages []string
}

func (a *App) customerList(w http.ResponseWriter, r *http.Request) {
	rows, err := a.db.Query(`SELECT id, number, name, currency, hourly_rate FROM customer WHERE archived = 0 ORDER BY id`)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	var customers []customer
	for rows.Next() {
		var c customer
		var rate int64
		if err := rows.Scan(&c.ID, &c.Number, &c.Name, &c.Currency, &rate); err != nil {
			a.serverError(w, r, err)
			return
		}
		c.HourlyRate = formatAmount(rate, currency.MustParseISO(c.Currency))
		customers = append(customers, c)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "customers", customers)
}

func (a *App) customerNew(w http.ResponseWriter, r *http.Request) {
	d := a.cfg.Defaults
	a.renderCustomer(w, r, http.StatusOK, customer{Currency: d.Currency, InvoiceLanguage: d.Language, Timezone: d.Timezone}, nil)
}

func (a *App) renderCustomer(w http.ResponseWriter, r *http.Request, status int, c customer, errs []string) {
	langs := make([]string, len(languages))
	for i, tag := range languages {
		langs[i] = tag.String()
	}
	a.render(w, r, status, "customer", customerPage{Customer: c, Errors: errs, Languages: langs})
}

type storedRate struct {
	minor int64
	unit  currency.Unit
}

// customerFromForm returns the submitted customer, its rate in minor units and
// any validation errors as catalog keys. A rate submitted as prev displays is
// prev rescaled into the submitted currency.
func customerFromForm(r *http.Request, prev *storedRate) (customer, int64, []string) {
	c := customer{
		Name:            strings.TrimSpace(r.PostFormValue("name")),
		Address:         strings.TrimSpace(r.PostFormValue("address")),
		Email:           strings.TrimSpace(r.PostFormValue("email")),
		Currency:        strings.ToUpper(strings.TrimSpace(r.PostFormValue("currency"))),
		InvoiceLanguage: r.PostFormValue("invoice_language"),
		Timezone:        strings.TrimSpace(r.PostFormValue("timezone")),
		HourlyRate:      strings.TrimSpace(r.PostFormValue("hourly_rate")),
		Note:            strings.TrimSpace(r.PostFormValue("note")),
	}
	var errs []string
	if c.Name == "" {
		errs = append(errs, "customers.error.name")
	}
	if !supportedLanguage(c.InvoiceLanguage) {
		errs = append(errs, "customers.error.invoice_language")
	}
	if _, err := time.LoadLocation(c.Timezone); err != nil || c.Timezone == "" {
		errs = append(errs, "customers.error.timezone")
	}
	var rate int64
	cur, err := currency.ParseISO(c.Currency)
	switch {
	case err != nil:
		errs = append(errs, "customers.error.currency")
	case c.HourlyRate == "":
		errs = append(errs, "customers.error.hourly_rate_required")
	case prev != nil && c.HourlyRate == formatAmount(prev.minor, prev.unit):
		rate = rescaleAmount(prev.minor, prev.unit, cur)
	default:
		if rate, err = parseAmount(c.HourlyRate, cur); err != nil {
			errs = append(errs, "customers.error.hourly_rate")
		}
	}
	return c, rate, errs
}

func (a *App) customerCreate(w http.ResponseWriter, r *http.Request) {
	c, rate, errs := customerFromForm(r, nil)
	if errs != nil {
		a.renderCustomer(w, r, http.StatusUnprocessableEntity, c, errs)
		return
	}
	if err := a.insertCustomer(c, rate); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/customers", http.StatusSeeOther)
}

// insertCustomer draws the number in the same transaction, so a failed insert never consumes one.
func (a *App) insertCustomer(c customer, rate int64) error {
	tx, err := a.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	var seq int64
	if err := tx.QueryRow(`UPDATE sequence SET last_value = last_value + 1
		WHERE name = 'customer' AND year IS NULL RETURNING last_value`).Scan(&seq); err != nil {
		return err
	}
	number, err := formatCustomerNumber(a.cfg.CustomerNumberFormat, seq)
	if err != nil {
		return err
	}
	if _, err := tx.Exec(`INSERT INTO customer
		(number, name, address, email, currency, invoice_language, timezone, hourly_rate, note)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		number, c.Name, c.Address, c.Email, c.Currency, c.InvoiceLanguage, c.Timezone, rate, c.Note); err != nil {
		return err
	}
	return tx.Commit()
}

func (a *App) customerEdit(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		http.NotFound(w, r)
		return
	}
	c := customer{ID: id}
	var rate int64
	err = a.db.QueryRow(`SELECT number, name, address, email, currency, invoice_language, timezone, hourly_rate, note
		FROM customer WHERE id = ?`, id).
		Scan(&c.Number, &c.Name, &c.Address, &c.Email, &c.Currency, &c.InvoiceLanguage, &c.Timezone, &rate, &c.Note)
	if errors.Is(err, sql.ErrNoRows) {
		http.NotFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	c.HourlyRate = formatAmount(rate, currency.MustParseISO(c.Currency))
	a.renderCustomer(w, r, http.StatusOK, c, nil)
}

func (a *App) customerUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		http.NotFound(w, r)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var number, oldCurrency string
	var oldRate int64
	err = tx.QueryRow(`SELECT number, currency, hourly_rate FROM customer WHERE id = ?`, id).Scan(&number, &oldCurrency, &oldRate)
	if errors.Is(err, sql.ErrNoRows) {
		http.NotFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	prev := storedRate{oldRate, currency.MustParseISO(oldCurrency)}
	c, rate, errs := customerFromForm(r, &prev)
	c.ID, c.Number = id, number
	if errs != nil {
		_ = tx.Rollback()
		a.renderCustomer(w, r, http.StatusUnprocessableEntity, c, errs)
		return
	}
	if err := updateCustomer(tx, c, rate, prev.unit); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/customers", http.StatusSeeOther)
}

// updateCustomer rescales the customer's project rate overrides when its currency changes from old.
func updateCustomer(tx *sql.Tx, c customer, rate int64, old currency.Unit) error {
	if _, err := tx.Exec(`UPDATE customer SET name = ?, address = ?, email = ?, currency = ?,
		invoice_language = ?, timezone = ?, hourly_rate = ?, note = ? WHERE id = ?`,
		c.Name, c.Address, c.Email, c.Currency, c.InvoiceLanguage, c.Timezone, rate, c.Note, c.ID); err != nil {
		return err
	}
	unit := currency.MustParseISO(c.Currency)
	if unit != old {
		rows, err := tx.Query(`SELECT id, hourly_rate FROM project WHERE customer_id = ? AND hourly_rate IS NOT NULL`, c.ID)
		if err != nil {
			return err
		}
		rates := map[int64]int64{}
		for rows.Next() {
			var id, minor int64
			if err := rows.Scan(&id, &minor); err != nil {
				_ = rows.Close()
				return err
			}
			rates[id] = minor
		}
		if err := rows.Close(); err != nil {
			return err
		}
		if err := rows.Err(); err != nil {
			return err
		}
		for id, minor := range rates {
			if _, err := tx.Exec(`UPDATE project SET hourly_rate = ? WHERE id = ?`, rescaleAmount(minor, old, unit), id); err != nil {
				return err
			}
		}
	}
	return tx.Commit()
}
