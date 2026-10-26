package app

import (
	"maps"
	"slices"
	"time"

	"golang.org/x/text/currency"
)

// amountDue expects the issued invoice aliased i.
const amountDue = `i.gross + (SELECT ifnull(sum(c.gross), 0) FROM invoice c WHERE c.cancels_id = i.id)`

type openInvoice struct {
	ID                                             int64
	Number, Customer, DueDate, AmountDue, Currency string
	Overdue                                        bool
}

type currencyTotal struct {
	Currency, Open, Overdue string
}

type openInvoices struct {
	Rows   []openInvoice
	Totals []currencyTotal
}

func (a *App) loadOpenInvoices() (openInvoices, error) {
	var open openInvoices
	rows, err := a.db.Query(`SELECT i.id, i.number, ifnull(i.snapshot ->> '$.customer.name', c.name), i.due_date, c.timezone,
		i.currency, ` + amountDue + `
		FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE i.paid_date IS NULL AND ` + invoiceStatus + ` IN ('issued', 'partially_cancelled')
		ORDER BY i.due_date, i.number`)
	if err != nil {
		return open, err
	}
	defer func() { _ = rows.Close() }()
	now := a.now()
	type sums struct{ open, overdue int64 }
	totals := map[string]*sums{}
	locs := locationCache{}
	for rows.Next() {
		var inv openInvoice
		var tz string
		var due int64
		if err := rows.Scan(&inv.ID, &inv.Number, &inv.Customer, &inv.DueDate, &tz, &inv.Currency, &due); err != nil {
			return open, err
		}
		loc, err := locs.load(tz)
		if err != nil {
			return open, err
		}
		inv.Overdue = inv.DueDate < now.In(loc).Format(time.DateOnly)
		inv.AmountDue = formatAmount(due, currency.MustParseISO(inv.Currency))
		open.Rows = append(open.Rows, inv)
		s := totals[inv.Currency]
		if s == nil {
			s = &sums{}
			totals[inv.Currency] = s
		}
		s.open += due
		if inv.Overdue {
			s.overdue += due
		}
	}
	if err := rows.Err(); err != nil {
		return open, err
	}
	for _, code := range slices.Sorted(maps.Keys(totals)) {
		unit := currency.MustParseISO(code)
		open.Totals = append(open.Totals, currencyTotal{Currency: code,
			Open: formatAmount(totals[code].open, unit), Overdue: formatAmount(totals[code].overdue, unit)})
	}
	return open, nil
}
