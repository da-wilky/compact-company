package app

import (
	"database/sql"
	"net/http"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /week", a.authed(a.week))
	})
}

// weekRow is one project of the week; Project is "none" for the Unassigned row.
type weekRow struct {
	Project string
	Name    string
	Days    [7]time.Duration
	Total   time.Duration
}

type weekDay struct{ Weekday, Date string }

var weekdays = [7]string{"week.mon", "week.tue", "week.wed", "week.thu", "week.fri", "week.sat", "week.sun"}

type weekPage struct {
	Days       [7]weekDay
	Rows       []weekRow
	Totals     [7]time.Duration
	Total      time.Duration
	Prev, Next string
	Everyone   bool
}

func (a *App) week(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	day := a.now().In(loc)
	if s := r.FormValue("week"); s != "" {
		if day, err = time.ParseInLocation(dayLayout, s, loc); err != nil {
			http.Error(w, "invalid week", http.StatusBadRequest)
			return
		}
	}
	monday := time.Date(day.Year(), day.Month(), day.Day()-(int(day.Weekday())+6)%7, 0, 0, 0, 0, loc)
	var bounds [8]time.Time
	page := weekPage{Prev: monday.AddDate(0, 0, -7).Format(dayLayout), Next: monday.AddDate(0, 0, 7).Format(dayLayout), Everyone: r.FormValue("everyone") != ""}
	for i := range bounds {
		bounds[i] = time.Date(monday.Year(), monday.Month(), monday.Day()+i, 0, 0, 0, 0, loc)
		if i < 7 {
			page.Days[i] = weekDay{weekdays[i], bounds[i].Format(dayLayout)}
		}
	}
	rows, err := a.db.Query(`
		SELECT ifnull(e.project_id, 'none'), ifnull(c.name || ' · ' || p.name, ''), e.start_time, e.end_time
		FROM time_entry e LEFT JOIN project p ON p.id = e.project_id LEFT JOIN customer c ON c.id = p.customer_id
		WHERE (? OR e.user_id = ?) AND e.start_time >= ? AND e.start_time < ?
		ORDER BY e.project_id IS NULL, c.name, p.name, p.id`,
		page.Everyone, currentUser(r).ID, bounds[0].Unix(), bounds[7].Unix())
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var row weekRow
		var start int64
		var end sql.NullInt64
		if err := rows.Scan(&row.Project, &row.Name, &start, &end); err != nil {
			a.serverError(w, r, err)
			return
		}
		if n := len(page.Rows); n == 0 || page.Rows[n-1].Project != row.Project {
			page.Rows = append(page.Rows, row)
		}
		finish := a.now()
		if end.Valid {
			finish = time.Unix(end.Int64, 0)
		}
		d := finish.Sub(time.Unix(start, 0))
		i := 6
		for i > 0 && start < bounds[i].Unix() {
			i--
		}
		last := &page.Rows[len(page.Rows)-1]
		last.Days[i] += d
		last.Total += d
		page.Totals[i] += d
		page.Total += d
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "week", page)
}
