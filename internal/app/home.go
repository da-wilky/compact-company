package app

import (
	"database/sql"
	"fmt"
	"net/http"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /{$}", a.authed(a.home))
	})
}

func formatHours(d time.Duration) string {
	return fmt.Sprintf("%.1f", d.Hours())
}

type hoursTotal struct {
	Total, Billable time.Duration
}

type overviewPage struct {
	Week, Month     hoursTotal
	Unassigned      int
	UnassignedHours time.Duration
	Unbilled        []unbilledCustomer
	Drafts          []draftSummary
}

func (p overviewPage) Empty() bool {
	return p.Unassigned == 0 && len(p.Unbilled) == 0 && len(p.Drafts) == 0
}

func (a *App) home(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	now := a.now()
	day := now.In(loc)
	weekStart := time.Date(day.Year(), day.Month(), day.Day()-(int(day.Weekday())+6)%7, 0, 0, 0, 0, loc)
	weekEnd := weekStart.AddDate(0, 0, 7)
	monthStart := time.Date(day.Year(), day.Month(), 1, 0, 0, 0, 0, loc)
	monthEnd := monthStart.AddDate(0, 1, 0)
	var page overviewPage
	rows, err := a.db.Query(`
		SELECT start_time, end_time, ifnull(billable, 0) FROM time_entry
		WHERE user_id = ? AND start_time >= ? AND start_time < ?`,
		currentUser(r).ID, min(weekStart.Unix(), monthStart.Unix()), max(weekEnd.Unix(), monthEnd.Unix()))
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var start int64
		var end sql.NullInt64
		var billable bool
		if err := rows.Scan(&start, &end, &billable); err != nil {
			a.serverError(w, r, err)
			return
		}
		finish := now
		if end.Valid {
			finish = time.Unix(end.Int64, 0)
		}
		d := finish.Sub(time.Unix(start, 0))
		for _, p := range []struct {
			total    *hoursTotal
			from, to time.Time
		}{{&page.Week, weekStart, weekEnd}, {&page.Month, monthStart, monthEnd}} {
			if start >= p.from.Unix() && start < p.to.Unix() {
				p.total.Total += d
				if billable {
					p.total.Billable += d
				}
			}
		}
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	var seconds int64
	if err := a.db.QueryRow(`
		SELECT count(*), ifnull(sum(end_time - start_time), 0) FROM time_entry
		WHERE user_id = ? AND project_id IS NULL AND end_time IS NOT NULL`,
		currentUser(r).ID).Scan(&page.Unassigned, &seconds); err != nil {
		a.serverError(w, r, err)
		return
	}
	page.UnassignedHours = time.Duration(seconds) * time.Second
	if page.Unbilled, err = a.unbilledCustomers(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if page.Drafts, err = a.drafts(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "home", page)
}
