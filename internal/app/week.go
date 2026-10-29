package app

import (
	"database/sql"
	"net/http"
	"net/url"
	"slices"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /week", a.authed(a.week))
	})
}

// weekRow is one project of the week; Project is "none" for the Unassigned row.
type weekRow struct {
	Project  string
	Name     string
	Days     [7]time.Duration
	Total    time.Duration
	Invoices []invoiceRef
}

type weekCustomer struct {
	id       int64
	Name     string
	Projects []weekRow
}

type weekDay struct {
	Weekday, Date string
	Today         bool
	Time          time.Time
}

var weekdays = [7]string{"week.mon", "week.tue", "week.wed", "week.thu", "week.fri", "week.sat", "week.sun"}

type weekPage struct {
	Days       [7]weekDay
	Customers  []weekCustomer
	Unassigned weekRow
	Totals     [7]time.Duration
	Total      time.Duration
	Prev, Next string
	Current    bool
	Everyone   bool
	Whose      []filterOption
}

func mondayOf(day time.Time) time.Time {
	return time.Date(day.Year(), day.Month(), day.Day()-(int(day.Weekday())+6)%7, 0, 0, 0, 0, day.Location())
}

func (a *App) week(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	day := a.now().In(loc)
	today := day.Format(dayLayout)
	if s := r.FormValue("week"); s != "" {
		if day, err = time.ParseInLocation(dayLayout, s, loc); err != nil {
			a.errorPage(w, r, http.StatusBadRequest, "error.invalid_week")
			return
		}
	}
	monday := mondayOf(day)
	var bounds [8]time.Time
	page := weekPage{Prev: monday.AddDate(0, 0, -7).Format(dayLayout), Next: monday.AddDate(0, 0, 7).Format(dayLayout), Everyone: r.FormValue("everyone") != ""}
	page.Unassigned.Project = "none"
	week := url.Values{"week": {monday.Format(dayLayout)}}
	team := url.Values{"week": {monday.Format(dayLayout)}, "everyone": {"1"}}
	page.Whose = []filterOption{{"filter.mine", "/week?" + week.Encode(), !page.Everyone}, {"filter.team", "/week?" + team.Encode(), page.Everyone}}
	for i := range bounds {
		bounds[i] = time.Date(monday.Year(), monday.Month(), monday.Day()+i, 0, 0, 0, 0, loc)
		if i < 7 {
			date := bounds[i].Format(dayLayout)
			page.Days[i] = weekDay{weekdays[i], date, date == today, bounds[i]}
			page.Current = page.Current || date == today
		}
	}
	rows, err := a.db.Query(`
		SELECT ifnull(e.project_id, 'none'), ifnull(c.id, 0), ifnull(c.name, ''), ifnull(p.name, ''), e.start_time, e.end_time, ci.id, ci.number
		FROM time_entry e LEFT JOIN project p ON p.id = e.project_id LEFT JOIN customer c ON c.id = p.customer_id`+claimJoin+`
		WHERE (? OR e.user_id = ?) AND e.start_time >= ? AND e.start_time < ?
		ORDER BY e.project_id IS NULL, c.name, c.id, p.name, p.id, ci.id`,
		page.Everyone, currentUser(r).ID, bounds[0].Unix(), bounds[7].Unix())
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var project, customer, name string
		var customerID, start int64
		var end, invoice sql.NullInt64
		var number sql.NullString
		if err := rows.Scan(&project, &customerID, &customer, &name, &start, &end, &invoice, &number); err != nil {
			a.serverError(w, r, err)
			return
		}
		row := &page.Unassigned
		if project != "none" {
			if n := len(page.Customers); n == 0 || page.Customers[n-1].id != customerID {
				page.Customers = append(page.Customers, weekCustomer{id: customerID, Name: customer})
			}
			c := &page.Customers[len(page.Customers)-1]
			if n := len(c.Projects); n == 0 || c.Projects[n-1].Project != project {
				c.Projects = append(c.Projects, weekRow{Project: project, Name: name})
			}
			row = &c.Projects[len(c.Projects)-1]
		}
		if ref := invoiceRefOrNil(invoice, number); ref != nil && !slices.Contains(row.Invoices, *ref) {
			row.Invoices = append(row.Invoices, *ref)
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
		row.Days[i] += d
		row.Total += d
		page.Totals[i] += d
		page.Total += d
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "week", page)
}
