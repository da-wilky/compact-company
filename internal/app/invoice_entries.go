package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/refresh", a.authed(a.refreshConfirm))
		mux.HandleFunc("POST /invoices/{id}/refresh", a.authed(a.refresh))
	})
}

func (a *App) refreshConfirm(w http.ResponseWriter, r *http.Request) {
	if d, ok := a.draftFromPath(w, r); ok {
		a.render(w, r, http.StatusOK, "invoice_refresh", draftPage{Draft: d})
	}
}

// refresh replaces the draft's generated lines; deleting them releases their entries first.
func (a *App) refresh(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	_, err = tx.Exec(`DELETE FROM invoice_line WHERE generated = 1
		AND invoice_id IN (SELECT id FROM invoice WHERE id = ? AND number IS NULL)`, id)
	if err == nil {
		err = pullEntries(tx, id)
	}
	if err == nil {
		err = tx.Commit()
	}
	switch {
	case errors.Is(err, sql.ErrNoRows):
		a.notFound(w, r)
	case err != nil:
		a.serverError(w, r, err)
	default:
		http.Redirect(w, r, draftPath(id), http.StatusSeeOther)
	}
}

// invoiceRef is the invoice an entry is claimed by; Number is "" while it is a draft.
type invoiceRef struct {
	ID     int64
	Number string
}

// claimJoin joins time_entry e to the invoice claiming it as ci.
const claimJoin = ` LEFT JOIN invoice_line cl ON cl.id = e.invoice_line_id LEFT JOIN invoice ci ON ci.id = cl.invoice_id`

func claimedBy(id sql.NullInt64, number sql.NullString) *invoiceRef {
	if !id.Valid {
		return nil
	}
	return &invoiceRef{ID: id.Int64, Number: number.String}
}

type pulledLine struct {
	description, detail string
	seconds, unitPrice  int64
	entries             []int64
}

// pullEntries generates lines on draft id from its customer's billable, assigned, ended and unclaimed
// entries starting in its range, and claims those entries. It returns sql.ErrNoRows unless id is a draft.
func pullEntries(tx *sql.Tx, id int64) error {
	var from, to, grouping string
	var rounding int64
	if err := tx.QueryRow(`SELECT range_from, range_to, grouping, rounding_minutes
		FROM invoice WHERE id = ? AND kind = ? AND number IS NULL`, id, kindInvoice).
		Scan(&from, &to, &grouping, &rounding); err != nil {
		return err
	}
	loc, err := customerLocation(tx, id)
	if err != nil {
		return err
	}
	start, err := time.ParseInLocation(time.DateOnly, from, loc)
	if err != nil {
		return err
	}
	end, err := time.ParseInLocation(time.DateOnly, to, loc)
	if err != nil {
		return err
	}
	order := `p.name, p.id, e.start_time, e.id`
	if grouping == "entry" {
		order = `e.start_time, e.id`
	}
	rows, err := tx.Query(`SELECT e.id, p.id, p.name, ifnull(p.hourly_rate, c.hourly_rate), e.start_time, e.end_time - e.start_time, e.description
		FROM time_entry e JOIN project p ON p.id = e.project_id JOIN customer c ON c.id = p.customer_id
		JOIN invoice i ON i.customer_id = c.id
		WHERE i.id = ? AND e.billable = 1 AND e.end_time IS NOT NULL AND e.invoice_line_id IS NULL
		AND e.start_time >= ? AND e.start_time < ?
		ORDER BY `+order, id, start.Unix(), end.AddDate(0, 0, 1).Unix())
	if err != nil {
		return err
	}
	var lines []pulledLine
	lastProject := int64(-1)
	for rows.Next() {
		var entryID, projectID, rate, startTime, seconds int64
		var project, description string
		if err := rows.Scan(&entryID, &projectID, &project, &rate, &startTime, &seconds, &description); err != nil {
			_ = rows.Close()
			return err
		}
		switch {
		case grouping == "entry":
			if description == "" {
				description = project
			}
			lines = append(lines, pulledLine{description: description, detail: time.Unix(startTime, 0).In(loc).Format(time.DateOnly), unitPrice: rate})
		case projectID != lastProject:
			lines = append(lines, pulledLine{description: project, unitPrice: rate})
		}
		lastProject = projectID
		l := &lines[len(lines)-1]
		l.seconds += seconds
		l.entries = append(l.entries, entryID)
	}
	if err := rows.Close(); err != nil {
		return err
	}
	if err := rows.Err(); err != nil {
		return err
	}
	for _, l := range lines {
		var lineID int64
		if err := tx.QueryRow(`INSERT INTO invoice_line (invoice_id, position, generated, description, detail, quantity, unit, unit_price)
			VALUES (?1, (SELECT ifnull(max(position), 0) + 1 FROM invoice_line WHERE invoice_id = ?1), 1, ?2, ?3, ?4, 'h', ?5) RETURNING id`,
			id, l.description, l.detail, generatedQuantity(l.seconds, rounding), l.unitPrice).Scan(&lineID); err != nil {
			return err
		}
		for _, e := range l.entries {
			if _, err := tx.Exec(`UPDATE time_entry SET invoice_line_id = ? WHERE id = ?`, lineID, e); err != nil {
				return err
			}
		}
	}
	return nil
}

// lineEntry is an entry claimed by a line; Date is ISO in the customer's timezone.
type lineEntry struct {
	Date        string `json:"date"`
	User        string `json:"user"`
	Seconds     int64  `json:"seconds"`
	Description string `json:"description"`
}

func (e lineEntry) Duration() time.Duration {
	return time.Duration(e.Seconds) * time.Second
}

// lineEntries returns the entries claimed by invoice id's lines, by line id, in start order.
func (a *App) lineEntries(q querier, id int64) (map[int64][]lineEntry, error) {
	loc, err := customerLocation(q, id)
	if err != nil {
		return nil, err
	}
	rows, err := q.Query(`SELECT l.id, e.start_time, u.display_name, e.end_time - e.start_time, e.description
		FROM invoice_line l JOIN time_entry e ON e.invoice_line_id = l.id JOIN user u ON u.id = e.user_id
		WHERE l.invoice_id = ? ORDER BY e.start_time, e.id`, id)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	entries := map[int64][]lineEntry{}
	for rows.Next() {
		var lineID, start int64
		var e lineEntry
		if err := rows.Scan(&lineID, &start, &e.User, &e.Seconds, &e.Description); err != nil {
			return nil, err
		}
		e.Date = time.Unix(start, 0).In(loc).Format(time.DateOnly)
		entries[lineID] = append(entries[lineID], e)
	}
	return entries, rows.Err()
}

// generatedQuantity rounds seconds to the nearest increment, then to hours with 2 decimals, in thousandths.
func generatedQuantity(seconds, roundingMinutes int64) int64 {
	if inc := roundingMinutes * 60; inc > 0 {
		seconds = divRound(seconds, inc) * inc
	}
	return divRound(seconds*100, 3600) * 10
}
