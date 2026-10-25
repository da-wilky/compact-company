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

// customerLocation is the timezone of invoice id's customer.
func customerLocation(q querier, id int64) (*time.Location, error) {
	var tz string
	if err := q.QueryRow(`SELECT c.timezone FROM invoice i JOIN customer c ON c.id = i.customer_id WHERE i.id = ?`, id).Scan(&tz); err != nil {
		return nil, err
	}
	return time.LoadLocation(tz)
}

// issueDate is today in the customer's timezone of invoice id.
func (a *App) issueDate(q querier, id int64) (string, error) {
	loc, err := customerLocation(q, id)
	if err != nil {
		return "", err
	}
	return a.now().In(loc).Format(time.DateOnly), nil
}

// invoiceJSON builds the template input for invoice id, and returns its gross in minor units; a nil number makes it a draft.
// The bytes are what a snapshot stores, so a rendering can be reproduced from it.
func (a *App) invoiceJSON(q querier, id int64, number *string, issueDate string) ([]byte, int64, error) {
	b, err := a.loadBusiness()
	if err != nil {
		return nil, 0, err
	}
	if missing := missingBusinessFields(b); missing != nil {
		return nil, 0, &missingBusinessError{fields: missing}
	}
	d := invoiceData{
		Version:   1,
		Draft:     number == nil,
		Number:    number,
		IssueDate: issueDate,
		Business: businessData{Name: b.Name, Company: b.Company, Address: b.Address, Email: b.Email, Web: b.Web,
			Phone: b.Phone, TaxNumber: b.TaxNumber, Bank: bankData{Name: b.Bank.Name, IBAN: b.Bank.IBAN, BIC: b.Bank.BIC}},
		Tax: taxData{SmallBusiness: b.TaxMode != config.TaxModeStandard},
	}
	if !d.Tax.SmallBusiness {
		d.Tax.VATRate = int64(b.VATRate)
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
		d.Lines = append(d.Lines, lineData{Description: l.Description, Detail: l.Detail, Quantity: l.Quantity, Unit: l.Unit,
			UnitPrice: l.UnitPrice, Total: l.Total, Entries: append([]lineEntry{}, entries[l.ID]...)})
	}
	data, err := json.Marshal(d)
	return data, t.Gross, err
}
