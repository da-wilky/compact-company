package app

import (
	"database/sql"
	"errors"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"golang.org/x/text/currency"

	"github.com/da-wilky/compact-company/internal/config"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices", a.authed(a.invoiceList))
		mux.HandleFunc("GET /invoices/new", a.authed(a.draftNew))
		mux.HandleFunc("POST /invoices", a.authed(a.draftCreate))
		mux.HandleFunc("GET /invoices/{id}", a.authed(a.invoiceShow))
		mux.HandleFunc("POST /invoices/{id}", a.authed(a.draftUpdate))
		mux.HandleFunc("POST /invoices/{id}/delete", a.authed(a.draftDelete))
	})
}

var groupings = []string{"project", "entry"}

// Invoice kinds, as the invoice table's CHECK constraint allows them.
const (
	kindInvoice      = "invoice"
	kindCancellation = "cancellation"
)

// draftVATPercent is the business's current VAT rate, which drafts are totalled at.
func (a *App) draftVATPercent() (int64, error) {
	b, err := a.loadBusiness()
	if err != nil || b.TaxMode != config.TaxModeStandard {
		return 0, err
	}
	return int64(b.VATRate), nil
}

type invoiceRow struct {
	ID        int64
	Number    string
	Customer  string
	IssueDate string
	PaidDate  string
	Gross     string
	Currency  string
	Status    string
}

type invoicesPage struct {
	Rows      []invoiceRow
	Customers []customerOption
	Customer  int64
	Status    string
	Statuses  []string
}

type customerOption struct {
	ID   int64
	Name string
}

var invoiceStatuses = []string{"draft", "issued", "paid", "cancelled"}

// StatusLink is the list URL for status, keeping the customer filter.
func (p invoicesPage) StatusLink(status string) string {
	q := url.Values{}
	if p.Customer != 0 {
		q.Set("customer", strconv.FormatInt(p.Customer, 10))
	}
	if status != "" {
		q.Set("status", status)
	}
	if len(q) == 0 {
		return "/invoices"
	}
	return "/invoices?" + q.Encode()
}

func (a *App) invoiceList(w http.ResponseWriter, r *http.Request) {
	page := invoicesPage{Statuses: invoiceStatuses}
	page.Customer, _ = strconv.ParseInt(r.FormValue("customer"), 10, 64)
	if s := r.FormValue("status"); slices.Contains(invoiceStatuses, s) {
		page.Status = s
	}
	var err error
	if page.Customers, err = a.customerOptions(); err != nil {
		a.serverError(w, r, err)
		return
	}
	// Issued rows show their snapshot, so later customer or business changes don't reach them.
	rows, err := a.db.Query(`SELECT * FROM (SELECT i.id, ifnull(i.number, '') number, ifnull(i.snapshot ->> '$.customer.name', c.name),
		ifnull(i.issue_date, '') issue_date, ifnull(i.paid_date, '') paid_date, i.currency, i.gross,
		`+invoiceStatus+` status
		FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE (?1 = 0 OR i.customer_id = ?1))
		WHERE ?2 = '' OR ?2 = status OR (?2 = 'cancelled' AND status = 'partially_cancelled') OR (?2 = 'paid' AND paid_date != '')
		ORDER BY number != '', issue_date DESC, id DESC`, page.Customer, page.Status)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var row invoiceRow
		var gross sql.NullInt64
		if err := rows.Scan(&row.ID, &row.Number, &row.Customer, &row.IssueDate, &row.PaidDate, &row.Currency, &gross, &row.Status); err != nil {
			a.serverError(w, r, err)
			return
		}
		if gross.Valid {
			row.Gross = formatAmount(gross.Int64, currency.MustParseISO(row.Currency))
		}
		page.Rows = append(page.Rows, row)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	vat, err := a.draftVATPercent()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	for i := range page.Rows {
		row := &page.Rows[i]
		if row.Number != "" {
			continue
		}
		unit := currency.MustParseISO(row.Currency)
		_, totals, err := a.loadLines(row.ID, unit, vat)
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		row.Gross = formatAmount(totals.Gross, unit)
	}
	a.render(w, r, http.StatusOK, "invoices", page)
}

// customerOptions lists every customer, archived ones included: finished work can still be billed.
func (a *App) customerOptions() ([]customerOption, error) {
	rows, err := a.db.Query(`SELECT id, name FROM customer ORDER BY name, id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var opts []customerOption
	for rows.Next() {
		var o customerOption
		if err := rows.Scan(&o.ID, &o.Name); err != nil {
			return nil, err
		}
		opts = append(opts, o)
	}
	return opts, rows.Err()
}

type draftNewForm struct {
	Customer        string
	From, To        string
	Grouping        string
	RoundingMinutes string
	PaymentDays     string
}

type draftNewPage struct {
	Form       draftNewForm
	Customers  []customerOption
	Groupings  []string
	Increments []int
	Errors     []string
}

func (a *App) draftNew(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	now := a.now().In(loc)
	thisMonth := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, time.UTC)
	// The form selects only a customer it lists, so any other value selects nothing.
	a.renderDraftNew(w, r, http.StatusOK, draftNewForm{
		Customer:        r.FormValue("customer"),
		From:            thisMonth.AddDate(0, -1, 0).Format(time.DateOnly),
		To:              thisMonth.AddDate(0, 0, -1).Format(time.DateOnly),
		Grouping:        "project",
		RoundingMinutes: strconv.Itoa(a.cfg.Invoice.RoundingMinutes),
		PaymentDays:     strconv.Itoa(a.cfg.Invoice.PaymentDays),
	}, nil)
}

func (a *App) renderDraftNew(w http.ResponseWriter, r *http.Request, status int, f draftNewForm, errs []string) {
	customers, err := a.customerOptions()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "invoice_new", draftNewPage{Form: f, Customers: customers, Groupings: groupings, Increments: config.RoundingIncrements, Errors: errs})
}

func validOptionalRange(from, to string) bool {
	if from == "" && to == "" {
		return true
	}
	f, errF := time.Parse(time.DateOnly, from)
	t, errT := time.Parse(time.DateOnly, to)
	return errF == nil && errT == nil && !t.Before(f)
}

func parsePaymentDays(s string) (int, bool) {
	n, err := strconv.Atoi(strings.TrimSpace(s))
	return n, err == nil && n > 0
}

func (a *App) draftCreate(w http.ResponseWriter, r *http.Request) {
	f := draftNewForm{
		Customer:        r.PostFormValue("customer"),
		From:            strings.TrimSpace(r.PostFormValue("from")),
		To:              strings.TrimSpace(r.PostFormValue("to")),
		Grouping:        r.PostFormValue("grouping"),
		RoundingMinutes: r.PostFormValue("rounding_minutes"),
		PaymentDays:     strings.TrimSpace(r.PostFormValue("payment_days")),
	}
	var errs []string
	customerID, err := strconv.ParseInt(f.Customer, 10, 64)
	if err != nil {
		errs = append(errs, "invoices.error.customer")
	}
	if !validOptionalRange(f.From, f.To) {
		errs = append(errs, "invoices.error.range")
	}
	if !slices.Contains(groupings, f.Grouping) {
		errs = append(errs, "invoices.error.grouping")
	}
	rounding, err := strconv.Atoi(f.RoundingMinutes)
	if err != nil || !slices.Contains(config.RoundingIncrements, rounding) {
		errs = append(errs, "invoices.error.rounding")
	}
	paymentDays, ok := parsePaymentDays(f.PaymentDays)
	if !ok {
		errs = append(errs, "invoices.error.payment_days")
	}
	if errs != nil {
		a.renderDraftNew(w, r, http.StatusUnprocessableEntity, f, errs)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var id int64
	err = tx.QueryRow(`INSERT INTO invoice
		(kind, customer_id, range_from, range_to, service_from, service_to, grouping, rounding_minutes, payment_days, currency)
		SELECT ?7, id, ?1, ?2, '', '', ?3, ?4, ?5, currency FROM customer WHERE id = ?6 RETURNING id`,
		f.From, f.To, f.Grouping, rounding, paymentDays, customerID, kindInvoice).Scan(&id)
	if errors.Is(err, sql.ErrNoRows) {
		a.renderDraftNew(w, r, http.StatusUnprocessableEntity, f, []string{"invoices.error.customer"})
		return
	}
	if err == nil {
		err = pullEntries(tx, id)
	}
	if err == nil {
		err = tx.Commit()
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, draftPath(id), http.StatusSeeOther)
}

func draftPath(id int64) string {
	return "/invoices/" + strconv.FormatInt(id, 10)
}

// draftHeader is what stays editable on a draft after creation.
type draftHeader struct {
	From, To               string
	ServiceFrom, ServiceTo string
	PaymentDays            string
}

type draft struct {
	ID              int64
	CustomerNumber  string
	CustomerName    string
	Currency        string
	Grouping        string
	RoundingMinutes int
	Header          draftHeader
	// ServicePeriod is set while the draft claims entries, which then decide it.
	ServicePeriod   *period
	Lines           []invoiceLine
	Net, VAT, Gross string
	VATPercent      int64
	unit            currency.Unit
}

type draftPage struct {
	Draft  draft
	Errors []string
	renderFailure
}

// loadDraft returns sql.ErrNoRows unless id is a draft.
func (a *App) loadDraft(id int64) (draft, error) {
	d := draft{ID: id}
	var paymentDays int
	err := a.db.QueryRow(`SELECT c.number, c.name, i.currency, i.grouping, i.rounding_minutes,
		i.range_from, i.range_to, i.service_from, i.service_to, i.payment_days
		FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE i.id = ? AND i.kind = ? AND i.number IS NULL`, id, kindInvoice).
		Scan(&d.CustomerNumber, &d.CustomerName, &d.Currency, &d.Grouping, &d.RoundingMinutes,
			&d.Header.From, &d.Header.To, &d.Header.ServiceFrom, &d.Header.ServiceTo, &paymentDays)
	if err != nil {
		return d, err
	}
	d.Header.PaymentDays = strconv.Itoa(paymentDays)
	if p, ok, err := entryPeriod(a.db, id); err != nil {
		return d, err
	} else if ok {
		d.ServicePeriod = &p
	}
	d.unit = currency.MustParseISO(d.Currency)
	if d.VATPercent, err = a.draftVATPercent(); err != nil {
		return d, err
	}
	lines, totals, err := a.loadLines(id, d.unit, d.VATPercent)
	if err != nil {
		return d, err
	}
	d.Lines = lines
	d.Net, d.VAT, d.Gross = formatAmount(totals.Net, d.unit), formatAmount(totals.VAT, d.unit), formatAmount(totals.Gross, d.unit)
	return d, nil
}

// draftFromPath writes 404 or 500 itself when ok is false.
func (a *App) draftFromPath(w http.ResponseWriter, r *http.Request) (draft, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return draft{}, false
	}
	d, err := a.loadDraft(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return d, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return d, false
	}
	return d, true
}

func (a *App) draftUpdate(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	d.Header.From = strings.TrimSpace(r.PostFormValue("from"))
	d.Header.To = strings.TrimSpace(r.PostFormValue("to"))
	d.Header.PaymentDays = strings.TrimSpace(r.PostFormValue("payment_days"))
	if d.ServicePeriod == nil {
		d.Header.ServiceFrom = strings.TrimSpace(r.PostFormValue("service_from"))
		d.Header.ServiceTo = strings.TrimSpace(r.PostFormValue("service_to"))
	}
	var errs []string
	if !validOptionalRange(d.Header.From, d.Header.To) {
		errs = append(errs, "invoices.error.range")
	}
	if !validOptionalRange(d.Header.ServiceFrom, d.Header.ServiceTo) {
		errs = append(errs, "invoices.error.service_period")
	}
	paymentDays, ok := parsePaymentDays(d.Header.PaymentDays)
	if !ok {
		errs = append(errs, "invoices.error.payment_days")
	}
	if errs != nil {
		a.render(w, r, http.StatusUnprocessableEntity, "invoice", draftPage{Draft: d, Errors: errs})
		return
	}
	res, err := a.db.Exec(`UPDATE invoice SET range_from = ?, range_to = ?, service_from = ?, service_to = ?, payment_days = ?
		WHERE id = ? AND number IS NULL`,
		d.Header.From, d.Header.To, d.Header.ServiceFrom, d.Header.ServiceTo, paymentDays, d.ID)
	a.afterInvoiceWrite(w, r, res, err, draftPath(d.ID))
}

// Deleting the draft cascades to its lines, which releases their entries.
func (a *App) draftDelete(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(`DELETE FROM invoice WHERE id = ? AND kind = ? AND number IS NULL`, id, kindInvoice)
	a.afterInvoiceWrite(w, r, res, err, "/invoices")
}

// afterInvoiceWrite redirects to done, or answers 404 when the write matched no row
// because the invoice (or line) is gone or no longer in the state the write requires.
func (a *App) afterInvoiceWrite(w http.ResponseWriter, r *http.Request, res sql.Result, err error, done string) {
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	n, err := res.RowsAffected()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if n == 0 {
		a.notFound(w, r)
		return
	}
	http.Redirect(w, r, done, http.StatusSeeOther)
}
