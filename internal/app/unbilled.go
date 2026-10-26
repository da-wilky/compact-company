package app

import (
	"cmp"
	"database/sql"
	"slices"
	"time"
)

type draftTime struct {
	ID    int64
	Hours time.Duration
}

// unbilledCustomer is a customer's ended billable time not on an issued invoice, as tracked.
type unbilledCustomer struct {
	ID          int64
	Name        string
	NotInvoiced time.Duration
	OnDrafts    []draftTime
	Oldest      string
	total       time.Duration
	oldest      int64
	timezone    string
}

func (a *App) unbilledCustomers() ([]unbilledCustomer, error) {
	rows, err := a.db.Query(`
		SELECT c.id, c.name, c.timezone, ci.id, sum(e.end_time - e.start_time), min(e.start_time)
		FROM time_entry e JOIN project p ON p.id = e.project_id JOIN customer c ON c.id = p.customer_id` + claimJoin + `
		WHERE e.billable = 1 AND e.end_time IS NOT NULL AND ci.number IS NULL
		GROUP BY c.id, ci.id ORDER BY ci.id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var customers []unbilledCustomer
	for rows.Next() {
		var row unbilledCustomer
		var draft sql.NullInt64
		var seconds int64
		if err := rows.Scan(&row.ID, &row.Name, &row.timezone, &draft, &seconds, &row.oldest); err != nil {
			return nil, err
		}
		i := slices.IndexFunc(customers, func(c unbilledCustomer) bool { return c.ID == row.ID })
		if i < 0 {
			customers = append(customers, row)
			i = len(customers) - 1
		}
		c := &customers[i]
		c.oldest = min(c.oldest, row.oldest)
		d := time.Duration(seconds) * time.Second
		c.total += d
		if draft.Valid {
			c.OnDrafts = append(c.OnDrafts, draftTime{draft.Int64, d})
		} else {
			c.NotInvoiced = d
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	for i := range customers {
		c := &customers[i]
		loc, err := time.LoadLocation(c.timezone)
		if err != nil {
			return nil, err
		}
		c.Oldest = time.Unix(c.oldest, 0).In(loc).Format(time.DateOnly)
	}
	slices.SortStableFunc(customers, func(x, y unbilledCustomer) int { return cmp.Compare(y.total, x.total) })
	return customers, nil
}

type draftSummary struct {
	ID       int64
	Customer string
	From, To string
}

func (a *App) drafts() ([]draftSummary, error) {
	rows, err := a.db.Query(`SELECT i.id, c.name, i.range_from, i.range_to FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE i.number IS NULL AND i.kind = '` + kindInvoice + `' ORDER BY c.name, i.range_from, i.id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var drafts []draftSummary
	for rows.Next() {
		var d draftSummary
		if err := rows.Scan(&d.ID, &d.Customer, &d.From, &d.To); err != nil {
			return nil, err
		}
		drafts = append(drafts, d)
	}
	return drafts, rows.Err()
}
