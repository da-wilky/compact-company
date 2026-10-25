package app

import (
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /entries", a.authed(a.entryList))
		mux.HandleFunc("GET /entries/new", a.authed(a.entryNew))
		mux.HandleFunc("POST /entries", a.authed(a.entryCreate))
		mux.HandleFunc("GET /entries/{id}", a.authed(a.entryEdit))
		mux.HandleFunc("POST /entries/{id}", a.authed(a.entryUpdate))
		mux.HandleFunc("POST /entries/{id}/delete", a.authed(a.entryDelete))
	})
}

const dayLayout = "2006-01-02"

var inputLayouts = []string{"2006-01-02T15:04", "2006-01-02T15:04:05"}

func formatDuration(d time.Duration) string {
	m := int64(d / time.Minute)
	return fmt.Sprintf("%d:%02d", m/60, m%60)
}

// entryRow is an entry as listings show it; Project is "customer · project", "" when unassigned.
type entryRow struct {
	ID          int64
	User        string
	Own         bool
	Project     string
	Start, End  time.Time
	Running     bool
	Duration    time.Duration
	Description string
	Billable    sql.NullBool
}

type entriesPage struct {
	Day, Prev, Next string
	Everyone        bool
	Project         string
	Self            string
	Entries         []entryRow
}

func (a *App) entryList(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	day := a.now().In(loc)
	if s := r.FormValue("day"); s != "" {
		if day, err = time.ParseInLocation(dayLayout, s, loc); err != nil {
			http.Error(w, "invalid day", http.StatusBadRequest)
			return
		}
	}
	from := time.Date(day.Year(), day.Month(), day.Day(), 0, 0, 0, 0, loc)
	to := from.AddDate(0, 0, 1)
	u := currentUser(r)
	everyone := r.FormValue("everyone") != ""
	project := r.FormValue("project")
	args := []any{everyone, u.ID, from.Unix(), to.Unix()}
	filter := ""
	switch project {
	case "":
	case "none":
		filter = " AND e.project_id IS NULL"
	default:
		id, err := strconv.ParseInt(project, 10, 64)
		if err != nil {
			http.Error(w, "invalid project", http.StatusBadRequest)
			return
		}
		filter = " AND e.project_id = ?"
		args = append(args, id)
	}
	rows, err := a.db.Query(`
		SELECT e.id, e.user_id, u.display_name, ifnull(c.name || ' · ' || p.name, ''), e.start_time, e.end_time, e.description, e.billable
		FROM time_entry e JOIN user u ON u.id = e.user_id
		LEFT JOIN project p ON p.id = e.project_id LEFT JOIN customer c ON c.id = p.customer_id
		WHERE (? OR e.user_id = ?) AND e.start_time >= ? AND e.start_time < ?`+filter+` ORDER BY e.start_time, e.id`,
		args...)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	page := entriesPage{Day: from.Format(dayLayout), Prev: from.AddDate(0, 0, -1).Format(dayLayout), Next: to.Format(dayLayout), Everyone: everyone, Project: project, Self: r.URL.RequestURI()}
	for rows.Next() {
		var e entryRow
		var userID, start int64
		var end sql.NullInt64
		if err := rows.Scan(&e.ID, &userID, &e.User, &e.Project, &start, &end, &e.Description, &e.Billable); err != nil {
			a.serverError(w, r, err)
			return
		}
		e.Own = userID == u.ID
		e.Start = time.Unix(start, 0)
		e.Running = !end.Valid
		e.End = a.now()
		if end.Valid {
			e.End = time.Unix(end.Int64, 0)
		}
		e.Duration = e.End.Sub(e.Start)
		page.Entries = append(page.Entries, e)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "entries", page)
}

// entryForm holds the submitted values as the form shows them; Billable is "", "yes" or "no" ("" = project default).
type entryForm struct {
	ID          int64
	Project     string
	Start, End  string
	Description string
	Billable    string
}

type entryPage struct {
	Entry     entryForm
	Customers []pickerCustomer
	Errors    []string
	ReadOnly  bool
	Return    string
}

func (a *App) renderEntry(w http.ResponseWriter, r *http.Request, status int, page entryPage) {
	var err error
	page.Return = r.FormValue("return")
	if page.Customers, err = a.loadPicker(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "entry", page)
}

func (a *App) entryNew(w http.ResponseWriter, r *http.Request) {
	a.renderEntry(w, r, http.StatusOK, entryPage{})
}

// savedEntry is a validated entry ready to be written.
type savedEntry struct {
	project    sql.NullInt64
	start, end int64
	billable   sql.NullBool
}

// validateEntry checks f inside tx and returns what to store, or validation errors as catalog keys.
func validateEntry(tx *sql.Tx, u *User, f entryForm, loc *time.Location) (savedEntry, []string, error) {
	var s savedEntry
	var errs []string
	start, err := parseInput(f.Start, loc)
	if err != nil {
		errs = append(errs, "entries.error.start")
	}
	end, err := parseInput(f.End, loc)
	if err != nil {
		errs = append(errs, "entries.error.end")
	}
	s.start, s.end = start.Unix(), end.Unix()
	if errs == nil && end.Before(start) {
		errs = append(errs, "entries.error.order")
	}
	if f.Billable != "" {
		s.billable = sql.NullBool{Bool: f.Billable == "yes", Valid: true}
	}
	if f.Project == "" {
		if s.billable.Valid {
			errs = append(errs, "entries.error.unassigned_billable")
		}
	} else if id, err := strconv.ParseInt(f.Project, 10, 64); err != nil {
		errs = append(errs, "entries.error.project")
	} else {
		def, err := activeProjectBillable(tx, id)
		if errors.Is(err, sql.ErrNoRows) {
			errs = append(errs, "entries.error.project")
		} else if err != nil {
			return s, nil, err
		}
		s.project = sql.NullInt64{Int64: id, Valid: true}
		if !s.billable.Valid {
			s.billable = sql.NullBool{Bool: def, Valid: true}
		}
	}
	if errs != nil {
		return s, errs, nil
	}
	conflict, err := entryConflicts(tx, u, f.ID, s.start, sql.NullInt64{Int64: s.end, Valid: true})
	if conflict {
		errs = append(errs, "entries.error.overlap")
	}
	return s, errs, err
}

func parseInput(s string, loc *time.Location) (time.Time, error) {
	var err error
	for _, layout := range inputLayouts {
		var t time.Time
		if t, err = time.ParseInLocation(layout, s, loc); err == nil {
			return t, nil
		}
	}
	return time.Time{}, err
}

func entryFromForm(r *http.Request) entryForm {
	return entryForm{
		Project:     r.PostFormValue("project"),
		Start:       r.PostFormValue("start"),
		End:         r.PostFormValue("end"),
		Description: strings.TrimSpace(r.PostFormValue("description")),
		Billable:    r.PostFormValue("billable"),
	}
}

func (a *App) entryCreate(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	u := currentUser(r)
	f := entryFromForm(r)
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	s, errs, err := validateEntry(tx, u, f, loc)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if errs != nil {
		a.renderEntry(w, r, http.StatusUnprocessableEntity, entryPage{Entry: f, Errors: errs})
		return
	}
	if _, err := tx.Exec(`INSERT INTO time_entry (user_id, project_id, start_time, end_time, description, billable) VALUES (?, ?, ?, ?, ?, ?)`,
		u.ID, s.project, s.start, s.end, f.Description, s.billable); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	redirectBack(w, r, time.Unix(s.start, 0).In(loc).Format(dayLayout))
}

func formatInput(t time.Time, loc *time.Location) string {
	t = t.In(loc)
	if t.Second() != 0 {
		return t.Format(inputLayouts[1])
	}
	return t.Format(inputLayouts[0])
}

// entryFromPath loads an ended entry and whether the current user may change it; it writes 404 or 500 itself when ok is false.
func (a *App) entryFromPath(w http.ResponseWriter, r *http.Request, loc *time.Location) (f entryForm, mine, ok bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		http.NotFound(w, r)
		return f, false, false
	}
	f.ID = id
	var userID, start, end int64
	var project, invoice sql.NullInt64
	var billable sql.NullBool
	err = a.db.QueryRow(`SELECT user_id, project_id, start_time, end_time, description, billable, invoice_id
		FROM time_entry WHERE id = ? AND end_time IS NOT NULL`, id).
		Scan(&userID, &project, &start, &end, &f.Description, &billable, &invoice)
	if errors.Is(err, sql.ErrNoRows) {
		http.NotFound(w, r)
		return f, false, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return f, false, false
	}
	if project.Valid {
		f.Project = strconv.FormatInt(project.Int64, 10)
	}
	f.Start, f.End = formatInput(time.Unix(start, 0), loc), formatInput(time.Unix(end, 0), loc)
	switch {
	case billable.Valid && billable.Bool:
		f.Billable = "yes"
	case billable.Valid:
		f.Billable = "no"
	}
	return f, userID == currentUser(r).ID && !invoice.Valid, true
}

func (a *App) entryEdit(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	f, mine, ok := a.entryFromPath(w, r, loc)
	if !ok {
		return
	}
	a.renderEntry(w, r, http.StatusOK, entryPage{Entry: f, ReadOnly: !mine})
}

func (a *App) entryUpdate(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	stored, mine, ok := a.entryFromPath(w, r, loc)
	if !ok {
		return
	}
	if !mine {
		forbidden(w)
		return
	}
	f := entryFromForm(r)
	f.ID = stored.ID
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	s, errs, err := validateEntry(tx, currentUser(r), f, loc)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if errs != nil {
		a.renderEntry(w, r, http.StatusUnprocessableEntity, entryPage{Entry: f, Errors: errs})
		return
	}
	if _, err := tx.Exec(`UPDATE time_entry SET project_id = ?, start_time = ?, end_time = ?, description = ?, billable = ? WHERE id = ?`,
		s.project, s.start, s.end, f.Description, s.billable, f.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	redirectBack(w, r, time.Unix(s.start, 0).In(loc).Format(dayLayout))
}

func forbidden(w http.ResponseWriter) {
	http.Error(w, http.StatusText(http.StatusForbidden), http.StatusForbidden)
}

func (a *App) entryDelete(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	f, mine, ok := a.entryFromPath(w, r, loc)
	if !ok {
		return
	}
	if !mine {
		forbidden(w)
		return
	}
	if _, err := a.db.Exec(`DELETE FROM time_entry WHERE id = ?`, f.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	redirectBack(w, r, f.Start[:len(dayLayout)])
}

// redirectBack returns to the listing the entry was opened from, else to the given day.
func redirectBack(w http.ResponseWriter, r *http.Request, day string) {
	to := r.PostFormValue("return")
	if !strings.HasPrefix(to, "/entries?") {
		to = "/entries?day=" + day
	}
	http.Redirect(w, r, to, http.StatusSeeOther)
}
