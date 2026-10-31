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

// quoteTotalsData holds each interval's totals by interval, nil for an interval without lines.
type quoteTotalsData map[string]*totalsData

// quoteTotalsOf taxes each interval on its own net, like an invoice.
func quoteTotalsOf(lineTotals map[string][]int64, vatPercent int64, unit currency.Unit) quoteTotalsData {
	out := quoteTotalsData{}
	for _, iv := range intervals {
		out[iv] = nil
		if len(lineTotals[iv]) > 0 {
			t := totalsOf(lineTotals[iv], vatPercent).format(unit)
			out[iv] = &t
		}
	}
	return out
}

// list is the totals of the intervals that have lines, in interval order.
func (t quoteTotalsData) list() []intervalTotals {
	var out []intervalTotals
	for _, iv := range intervals {
		if t[iv] != nil {
			out = append(out, intervalTotals{iv, *t[iv]})
		}
	}
	return out
}

// oneOff is the one-off gross, zero without one-off lines.
func (t quoteTotalsData) oneOff(unit currency.Unit) string {
	if t["once"] == nil {
		return formatAmount(0, unit)
	}
	return t["once"].Gross
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
	d.Totals = quoteTotalsOf(totals, tax.VATRate, unit)
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
