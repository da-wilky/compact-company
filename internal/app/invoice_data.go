package app

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"golang.org/x/text/currency"

	"github.com/da-wilky/compact-company/internal/config"
)

// querier is *sql.DB or *sql.Tx, so issuing can build its snapshot inside its transaction.
type querier interface {
	QueryRow(query string, args ...any) *sql.Row
	Query(query string, args ...any) (*sql.Rows, error)
}

// execer is *sql.DB or *sql.Tx, so a write can run alone or inside a larger transaction.
type execer interface {
	Exec(query string, args ...any) (sql.Result, error)
}

// invoiceData is the JSON handed to the Typst template, contract version 1 (templates/README.md).
type invoiceData struct {
	Version       int            `json:"version"`
	Kind          string         `json:"kind"`
	Draft         bool           `json:"draft"`
	Number        *string        `json:"number"`
	IssueDate     string         `json:"issue_date"`
	DueDate       string         `json:"due_date"`
	ServicePeriod period         `json:"service_period"`
	Language      string         `json:"language"`
	Currency      string         `json:"currency"`
	Business      businessData   `json:"business"`
	Tax           taxData        `json:"tax"`
	Customer      customerData   `json:"customer"`
	Lines         []lineData     `json:"lines"`
	Totals        totalsData     `json:"totals"`
	Cancels       *cancelledData `json:"cancels"`
	Reason        string         `json:"reason"`
}

type period struct {
	From string `json:"from"`
	To   string `json:"to"`
}

type businessData struct {
	Name      string   `json:"name"`
	Company   string   `json:"company"`
	Address   []string `json:"address"`
	Email     string   `json:"email"`
	Web       string   `json:"web"`
	Phone     string   `json:"phone"`
	TaxNumber string   `json:"tax_number"`
	Bank      bankData `json:"bank"`
}

type bankData struct {
	Name string `json:"name"`
	IBAN string `json:"iban"`
	BIC  string `json:"bic"`
}

type taxData struct {
	SmallBusiness bool  `json:"small_business"`
	VATRate       int64 `json:"vat_rate"`
}

type customerData struct {
	Number  string   `json:"number"`
	Name    string   `json:"name"`
	Address []string `json:"address"`
}

type lineData struct {
	Description string      `json:"description"`
	Detail      string      `json:"detail"`
	Quantity    string      `json:"quantity"`
	Unit        string      `json:"unit"`
	UnitPrice   string      `json:"unit_price"`
	Total       string      `json:"total"`
	Entries     []lineEntry `json:"entries"`
	Project     *string     `json:"project"`
}

type totalsData struct {
	Net   string `json:"net"`
	VAT   string `json:"vat"`
	Gross string `json:"gross"`
}

type cancelledData struct {
	Number    string `json:"number"`
	IssueDate string `json:"issue_date"`
}

// missingBusinessError refuses a render while business data an invoice requires is unset.
type missingBusinessError struct{ fields []string }

func (e *missingBusinessError) Error() string {
	return "missing business data: " + strings.Join(e.fields, ", ")
}

// SQL expressions for customerLocation selecting the customer of an invoice or quote id.
const (
	invoiceCustomer = `(SELECT customer_id FROM invoice WHERE id = ?)`
	quoteCustomer   = `(SELECT customer_id FROM quote WHERE id = ?)`
)

// customerLocation is the timezone of the customer whose id the SQL expression customer selects
// with args: "?" for a customer id, invoiceCustomer or quoteCustomer for a record's.
func customerLocation(q querier, customer string, args ...any) (*time.Location, error) {
	var tz string
	if err := q.QueryRow(`SELECT timezone FROM customer WHERE id = `+customer, args...).Scan(&tz); err != nil {
		return nil, err
	}
	return time.LoadLocation(tz)
}

// customerToday is today in the timezone customerLocation resolves.
func (a *App) customerToday(q querier, customer string, args ...any) (string, error) {
	loc, err := customerLocation(q, customer, args...)
	if err != nil {
		return "", err
	}
	return a.today(loc), nil
}

func (a *App) today(loc *time.Location) string {
	return a.now().In(loc).Format(time.DateOnly)
}

// entryPeriod runs from the start of the first entry claimed by invoice id to the end of its last;
// ok is false when it claims none.
func entryPeriod(q querier, id int64) (p period, ok bool, err error) {
	var first, last sql.NullInt64
	if err := q.QueryRow(`SELECT min(e.start_time), max(e.end_time)
		FROM time_entry e JOIN invoice_line l ON l.id = e.invoice_line_id WHERE l.invoice_id = ?`, id).
		Scan(&first, &last); err != nil || !first.Valid {
		return p, false, err
	}
	loc, err := customerLocation(q, invoiceCustomer, id)
	if err != nil {
		return p, false, err
	}
	// An entry ending at midnight did no work on the day that begins then.
	return period{
		From: time.Unix(first.Int64, 0).In(loc).Format(time.DateOnly),
		To:   time.Unix(last.Int64-1, 0).In(loc).Format(time.DateOnly),
	}, true, nil
}

type locationCache map[string]*time.Location

func (l locationCache) load(tz string) (*time.Location, error) {
	if loc, ok := l[tz]; ok {
		return loc, nil
	}
	loc, err := time.LoadLocation(tz)
	if err != nil {
		return nil, err
	}
	l[tz] = loc
	return loc, nil
}

// senderData is the business and its current tax as invoice and quote templates receive them;
// it refuses while business data a render requires is unset.
func (a *App) senderData() (businessData, taxData, error) {
	b, err := a.loadBusiness()
	if err != nil {
		return businessData{}, taxData{}, err
	}
	if missing := missingBusinessFields(b); missing != nil {
		return businessData{}, taxData{}, &missingBusinessError{fields: missing}
	}
	tax := taxData{SmallBusiness: b.TaxMode != config.TaxModeStandard}
	if !tax.SmallBusiness {
		tax.VATRate = int64(b.VATRate)
	}
	return businessData{Name: b.Name, Company: b.Company, Address: b.Address, Email: b.Email, Web: b.Web,
		Phone: b.Phone, TaxNumber: b.TaxNumber, Bank: bankData{Name: b.Bank.Name, IBAN: b.Bank.IBAN, BIC: b.Bank.BIC}}, tax, nil
}

// invoiceJSON builds the template input for invoice id, and returns its gross in minor units; a nil number makes it a draft.
// The bytes are what a snapshot stores, so a rendering can be reproduced from it.
func (a *App) invoiceJSON(q querier, id int64, number *string, issueDate string) ([]byte, int64, error) {
	business, tax, err := a.senderData()
	if err != nil {
		return nil, 0, err
	}
	d := invoiceData{
		Version:   1,
		Draft:     number == nil,
		Number:    number,
		IssueDate: issueDate,
		Business:  business,
		Tax:       tax,
	}
	var paymentDays int
	var address string
	var cancels cancelledData
	var cancelledTax taxData
	var isCancellation bool
	err = q.QueryRow(`SELECT i.kind, i.service_from, i.service_to, i.payment_days, i.reason, i.currency,
		c.number, c.name, c.address, c.invoice_language,
		i.cancels_id IS NOT NULL, ifnull(o.number, ''), ifnull(o.issue_date, ''),
		ifnull(o.snapshot ->> '$.tax.small_business', 0), ifnull(o.snapshot ->> '$.tax.vat_rate', 0)
		FROM invoice i JOIN customer c ON c.id = i.customer_id LEFT JOIN invoice o ON o.id = i.cancels_id
		WHERE i.id = ?`, id).
		Scan(&d.Kind, &d.ServicePeriod.From, &d.ServicePeriod.To, &paymentDays, &d.Reason, &d.Currency,
			&d.Customer.Number, &d.Customer.Name, &address, &d.Language,
			&isCancellation, &cancels.Number, &cancels.IssueDate, &cancelledTax.SmallBusiness, &cancelledTax.VATRate)
	if err != nil {
		return nil, 0, err
	}
	fromEntries, ok, err := entryPeriod(q, id)
	switch {
	case err != nil:
		return nil, 0, err
	case ok:
		d.ServicePeriod = fromEntries
	case d.ServicePeriod.From == "" || d.ServicePeriod.To == "":
		d.ServicePeriod = period{From: issueDate, To: issueDate}
	}
	if isCancellation {
		// A cancellation reverses exactly the tax that was charged, whatever the business's tax mode is now.
		d.Cancels, d.Tax = &cancels, cancelledTax
	}
	d.Customer.Address = addressLines(address)
	issued, err := time.Parse(time.DateOnly, issueDate)
	if err != nil {
		return nil, 0, fmt.Errorf("issue date: %w", err)
	}
	d.DueDate = issued.AddDate(0, 0, paymentDays).Format(time.DateOnly)
	unit := currency.MustParseISO(d.Currency)
	lines, totals, err := queryLines(q, id, unit)
	if err != nil {
		return nil, 0, err
	}
	t := totalsOf(totals, d.Tax.VATRate)
	d.Totals = totalsData{Net: formatAmount(t.Net, unit), VAT: formatAmount(t.VAT, unit), Gross: formatAmount(t.Gross, unit)}
	entries, err := a.lineEntries(q, id)
	if err != nil {
		return nil, 0, err
	}
	d.Lines = []lineData{}
	for _, l := range lines {
		line := lineData{Description: l.Description, Detail: l.Detail, Quantity: l.Quantity, Unit: l.Unit,
			UnitPrice: l.UnitPrice, Total: l.Total, Entries: append([]lineEntry{}, entries[l.ID]...)}
		if l.Project != "" {
			line.Project = &l.Project
		}
		d.Lines = append(d.Lines, line)
	}
	data, err := json.Marshal(d)
	return data, t.Gross, err
}
