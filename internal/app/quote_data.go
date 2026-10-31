package app

import (
	"encoding/json"

	"golang.org/x/text/currency"
)

// quoteData is the JSON handed to quote.typ, contract version 1 (templates/README.md).
type quoteData struct {
	Version    int                `json:"version"`
	Kind       string             `json:"kind"`
	Draft      bool               `json:"draft"`
	Number     *string            `json:"number"`
	IssueDate  string             `json:"issue_date"`
	ValidUntil string             `json:"valid_until"`
	Subject    string             `json:"subject"`
	Pricing    string             `json:"pricing"`
	Project    *string            `json:"project"`
	Language   string             `json:"language"`
	Currency   string             `json:"currency"`
	Business   businessData       `json:"business"`
	Tax        taxData            `json:"tax"`
	Customer   customerData       `json:"customer"`
	Lines      []quoteLineData    `json:"lines"`
	Sections   []quoteSectionData `json:"sections"`
	Totals     quoteTotalsData    `json:"totals"`
}

type quoteLineData struct {
	Description string `json:"description"`
	Detail      string `json:"detail"`
	Quantity    string `json:"quantity"`
	Unit        string `json:"unit"`
	UnitPrice   string `json:"unit_price"`
	Interval    string `json:"interval"`
	Total       string `json:"total"`
}

type quoteSectionData struct {
	Heading string `json:"heading"`
	Body    string `json:"body"`
}

// quoteTotalsData holds each interval's totals, nil for an interval without lines.
type quoteTotalsData struct {
	Once    *totalsData `json:"once"`
	Monthly *totalsData `json:"monthly"`
	Yearly  *totalsData `json:"yearly"`
}

// quoteJSON builds the template input for quote id; a nil number makes it a draft.
// The bytes are what a sent quote stores as its snapshot.
func (a *App) quoteJSON(q querier, id int64, number *string, issueDate string) ([]byte, error) {
	business, tax, err := a.senderData()
	if err != nil {
		return nil, err
	}
	d := quoteData{
		Version:   1,
		Kind:      "quote",
		Draft:     number == nil,
		Number:    number,
		IssueDate: issueDate,
		Business:  business,
		Tax:       tax,
	}
	var address string
	err = q.QueryRow(`SELECT q.valid_until, q.subject, q.pricing, p.name, q.language, q.currency, c.number, c.name, c.address
		FROM quote q JOIN customer c ON c.id = q.customer_id LEFT JOIN project p ON p.id = q.project_id
		WHERE q.id = ?`, id).
		Scan(&d.ValidUntil, &d.Subject, &d.Pricing, &d.Project, &d.Language, &d.Currency, &d.Customer.Number, &d.Customer.Name, &address)
	if err != nil {
		return nil, err
	}
	d.Customer.Address = addressLines(address)
	unit := currency.MustParseISO(d.Currency)
	lines, totals, err := queryQuoteLines(q, id, unit)
	if err != nil {
		return nil, err
	}
	d.Lines = []quoteLineData{}
	for _, l := range lines {
		d.Lines = append(d.Lines, quoteLineData{Description: l.Description, Detail: l.Detail, Quantity: l.Quantity,
			Unit: l.Unit, UnitPrice: l.UnitPrice, Interval: l.Interval, Total: l.Total})
	}
	intervalTotals := func(interval string) *totalsData {
		if len(totals[interval]) == 0 {
			return nil
		}
		t := totalsOf(totals[interval], tax.VATRate)
		return &totalsData{Net: formatAmount(t.Net, unit), VAT: formatAmount(t.VAT, unit), Gross: formatAmount(t.Gross, unit)}
	}
	d.Totals = quoteTotalsData{Once: intervalTotals("once"), Monthly: intervalTotals("monthly"), Yearly: intervalTotals("yearly")}
	sections, err := quoteSections(q, id)
	if err != nil {
		return nil, err
	}
	d.Sections = []quoteSectionData{}
	for _, s := range sections {
		d.Sections = append(d.Sections, quoteSectionData{Heading: s.Heading, Body: s.Body})
	}
	return json.Marshal(d)
}
