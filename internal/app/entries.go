package app

import (
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /entries", a.authed(a.entryList))
		mux.HandleFunc("GET /entries/new", a.authed(a.entryNew))
		mux.HandleFunc("POST /entries", a.authed(a.entryCreate))
		mux.HandleFunc("POST /entries/delete", a.authed(a.entryDeleteSelected))
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
	Invoice     *invoiceRef
}

// Selectable reports whether the viewer may select e for a mass action.
func (e entryRow) Selectable() bool {
	return e.Own && !e.Running && e.Invoice == nil
}

var entryPageSizes = []int{10, 25, 50, 100}

// entryPageSize reads the requested page size; anything not offered falls back to the first.
func entryPageSize(s string) int {
	n, _ := strconv.Atoi(s)
	if slices.Contains(entryPageSizes, n) {
		return n
	}
	return entryPageSizes[0]
}

type entryPageSizeLink struct {
	Size    int
	URL     string
	Current bool
}

type entryDay struct {
	Weekday   string
	Date      time.Time
	Total     time.Duration
	Continued bool
	Entries   []entryRow
}

// entriesFilter is what /entries shows: one Day ("" = every day), only the viewer's
// entries unless Everyone, an optional Project ("none" = unassigned), and PerPage entries at a time.
type entriesFilter struct {
	Day      string
	Everyone bool
	Project  string
	PerPage  int
}

func (f entriesFilter) url(before string) string {
	q := url.Values{}
	if f.Day != "" {
		q.Set("day", f.Day)
	}
	if f.Everyone {
		q.Set("everyone", "1")
	}
	if f.Project != "" {
		q.Set("project", f.Project)
	}
	if f.PerPage != entryPageSizes[0] {
		q.Set("per_page", strconv.Itoa(f.PerPage))
	}
	if before != "" {
		q.Set("before", before)
	}
	if len(q) == 0 {
		return "/entries"
	}
	return "/entries?" + q.Encode()
}

func (f entriesFilter) with(change func(*entriesFilter)) string {
	change(&f)
	return f.url("")
}

type entriesPage struct {
	entriesFilter
	From                  time.Time
	IsToday               bool
	ProjectName           string
	Self                  string
	Prev, Next, Today     string
	All, Mine, Team, Bare string
	More                  string
	DeleteURL             string
	PageSizes             []entryPageSizeLink
	Days                  []entryDay
	Selected              map[int64]bool
	Blocked               []entryRow
}

func (a *App) entryList(w http.ResponseWriter, r *http.Request) {
	a.listEntries(w, r, http.StatusOK, nil, nil)
}

// listEntries renders the listing r asks for, with the selection a refused mass action leaves behind.
func (a *App) listEntries(w http.ResponseWriter, r *http.Request, status int, selected map[int64]bool, blocked []entryRow) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	u := currentUser(r)
	f := entriesFilter{Everyone: r.FormValue("everyone") != "", Project: r.FormValue("project"), PerPage: entryPageSize(r.FormValue("per_page"))}
	where, args := " WHERE (? OR e.user_id = ?)", []any{f.Everyone, u.ID}
	page := entriesPage{Selected: selected, Blocked: blocked}
	switch f.Project {
	case "":
	case "none":
		where += " AND e.project_id IS NULL"
	default:
		id, err := strconv.ParseInt(f.Project, 10, 64)
		if err != nil {
			a.errorPage(w, r, http.StatusBadRequest, "error.invalid_project")
			return
		}
		err = a.db.QueryRow(`SELECT c.name || ' · ' || p.name FROM project p JOIN customer c ON c.id = p.customer_id WHERE p.id = ?`, id).Scan(&page.ProjectName)
		if errors.Is(err, sql.ErrNoRows) {
			a.errorPage(w, r, http.StatusBadRequest, "error.invalid_project")
			return
		}
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		where += " AND e.project_id = ?"
		args = append(args, id)
	}
	totalsWhere, totalsArgs := where, args
	today := a.now().In(loc).Format(dayLayout)
	limit := ""
	var before, cursorDay string
	if s := r.FormValue("day"); s != "" {
		day, err := time.ParseInLocation(dayLayout, s, loc)
		if err != nil {
			a.errorPage(w, r, http.StatusBadRequest, "error.invalid_day")
			return
		}
		f.Day = s
		page.From = day
		page.IsToday = s == today
		page.Prev = f.with(func(f *entriesFilter) { f.Day = day.AddDate(0, 0, -1).Format(dayLayout) })
		page.Next = f.with(func(f *entriesFilter) { f.Day = day.AddDate(0, 0, 1).Format(dayLayout) })
		page.Today = f.with(func(f *entriesFilter) { f.Day = today })
		page.All = f.with(func(f *entriesFilter) { f.Day = "" })
		where += " AND e.start_time >= ? AND e.start_time < ?"
		args = append(args, day.Unix(), day.AddDate(0, 0, 1).Unix())
	} else {
		if s := r.FormValue("before"); s != "" {
			start, id, err := parseCursor(s)
			if err != nil {
				a.errorPage(w, r, http.StatusBadRequest, "error.invalid_page")
				return
			}
			before = s
			cursorDay = time.Unix(start, 0).In(loc).Format(dayLayout)
			where += " AND (e.start_time, e.id) < (?, ?)"
			args = append(args, start, id)
		}
		limit = fmt.Sprintf(" LIMIT %d", f.PerPage+1)
		for _, n := range entryPageSizes {
			page.PageSizes = append(page.PageSizes, entryPageSizeLink{Size: n, URL: f.with(func(f *entriesFilter) { f.PerPage = n }), Current: n == f.PerPage})
		}
	}
	page.entriesFilter = f
	page.Self = f.url("")
	page.DeleteURL = "/entries/delete" + strings.TrimPrefix(f.url(before), "/entries")
	page.Mine = f.with(func(f *entriesFilter) { f.Everyone = false })
	page.Team = f.with(func(f *entriesFilter) { f.Everyone = true })
	page.Bare = f.with(func(f *entriesFilter) { f.Project = "" })

	rows, err := a.db.Query(`
		SELECT e.id, e.user_id, u.display_name, ifnull(c.name || ' · ' || p.name, ''), e.start_time, e.end_time, e.description, e.billable, ci.id, ci.number
		FROM time_entry e JOIN user u ON u.id = e.user_id
		LEFT JOIN project p ON p.id = e.project_id LEFT JOIN customer c ON c.id = p.customer_id`+claimJoin+where+`
		ORDER BY e.start_time DESC, e.id DESC`+limit, args...)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	var entries []entryRow
	for rows.Next() {
		var e entryRow
		var userID, start int64
		var end, invoice sql.NullInt64
		var number sql.NullString
		if err := rows.Scan(&e.ID, &userID, &e.User, &e.Project, &start, &end, &e.Description, &e.Billable, &invoice, &number); err != nil {
			a.serverError(w, r, err)
			return
		}
		e.Invoice = invoiceRefOrNil(invoice, number)
		e.Own = userID == u.ID
		e.Start = time.Unix(start, 0)
		e.Running = !end.Valid
		e.End = a.now()
		if end.Valid {
			e.End = time.Unix(end.Int64, 0)
		}
		e.Duration = e.End.Sub(e.Start)
		entries = append(entries, e)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if limit != "" && len(entries) > f.PerPage {
		entries = entries[:f.PerPage]
		last := entries[len(entries)-1]
		page.More = f.url(fmt.Sprintf("%d-%d", last.Start.Unix(), last.ID))
	}
	for _, e := range entries {
		start := e.Start.In(loc)
		date := time.Date(start.Year(), start.Month(), start.Day(), 0, 0, 0, 0, loc)
		if n := len(page.Days); n == 0 || !page.Days[n-1].Date.Equal(date) {
			page.Days = append(page.Days, entryDay{Weekday: weekdays[(int(date.Weekday())+6)%7], Date: date, Continued: n == 0 && date.Format(dayLayout) == cursorDay})
		}
		d := &page.Days[len(page.Days)-1]
		d.Entries = append(d.Entries, e)
	}
	if err := a.sumEntryDays(page.Days, totalsWhere, totalsArgs); err != nil {
		a.serverError(w, r, err)
		return
	}
	if r.Method == http.MethodGet && r.Header.Get("HX-Request") != "" && r.FormValue("before") != "" {
		a.execute(w, r, http.StatusOK, "entries", "entry-days", view{Data: page})
		return
	}
	a.render(w, r, status, "entries", page)
}

// sumEntryDays totals each day over all its matching entries, not only the ones on this page.
func (a *App) sumEntryDays(days []entryDay, where string, args []any) error {
	if len(days) == 0 {
		return nil
	}
	from, to := days[len(days)-1].Date, days[0].Date.AddDate(0, 0, 1)
	rows, err := a.db.Query(`SELECT e.start_time, e.end_time FROM time_entry e`+where+` AND e.start_time >= ? AND e.start_time < ?`,
		append(args, from.Unix(), to.Unix())...)
	if err != nil {
		return err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var start int64
		var end sql.NullInt64
		if err := rows.Scan(&start, &end); err != nil {
			return err
		}
		finish := a.now()
		if end.Valid {
			finish = time.Unix(end.Int64, 0)
		}
		for i := range days {
			if s := time.Unix(start, 0); !s.Before(days[i].Date) && s.Before(days[i].Date.AddDate(0, 0, 1)) {
				days[i].Total += finish.Sub(s)
			}
		}
	}
	return rows.Err()
}

func parseCursor(s string) (start, id int64, err error) {
	a, b, ok := strings.Cut(s, "-")
	if !ok {
		return 0, 0, strconv.ErrSyntax
	}
	if start, err = strconv.ParseInt(a, 10, 64); err != nil {
		return 0, 0, err
	}
	id, err = strconv.ParseInt(b, 10, 64)
	return start, id, err
}

// entryForm holds the submitted values as the form shows them; Billable is "", "yes" or "no" ("" = project default).
type entryForm struct {
	ID          int64
	Project     string
	Start, End  string
	Description string
	Billable    string
	Invoice     *invoiceRef
}

type entryPage struct {
	Entry     entryForm
	Customers []pickerCustomer
	Errors    []string
	ReadOnly  bool
	Return    string
}

// renderEntry offers storedProject ("" = none) even when it no longer receives new time.
func (a *App) renderEntry(w http.ResponseWriter, r *http.Request, status int, page entryPage, storedProject string) {
	storedID, _ := strconv.ParseInt(storedProject, 10, 64)
	var err error
	page.Return = r.FormValue("return")
	if page.Customers, err = a.loadPicker(storedID); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "entry", page)
}

func (a *App) entryNew(w http.ResponseWriter, r *http.Request) {
	a.renderEntry(w, r, http.StatusOK, entryPage{}, "")
}

type savedEntry struct {
	project    sql.NullInt64
	start, end int64
	billable   sql.NullBool
}

// validateEntry checks f inside tx and returns what to store, or validation errors as catalog keys.
// storedProject ("" = none) stays allowed after it was archived.
func validateEntry(tx *sql.Tx, u *User, f entryForm, loc *time.Location, storedProject string) (savedEntry, []string, error) {
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
		var def bool
		if f.Project == storedProject {
			err = tx.QueryRow(`SELECT billable FROM project WHERE id = ?`, id).Scan(&def)
		} else {
			def, err = activeProjectBillable(tx, id)
		}
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
	s, errs, err := validateEntry(tx, u, f, loc, "")
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if errs != nil {
		a.renderEntry(w, r, http.StatusUnprocessableEntity, entryPage{Entry: f, Errors: errs}, "")
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

// entryFromPath loads an ended entry and whether the current user owns it; it writes 404 or 500 itself when ok is false.
func (a *App) entryFromPath(w http.ResponseWriter, r *http.Request, loc *time.Location) (f entryForm, mine, ok bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return f, false, false
	}
	f.ID = id
	var userID, start, end int64
	var project, invoice sql.NullInt64
	var number sql.NullString
	var billable sql.NullBool
	err = a.db.QueryRow(`SELECT e.user_id, e.project_id, e.start_time, e.end_time, e.description, e.billable, ci.id, ci.number
		FROM time_entry e`+claimJoin+` WHERE e.id = ? AND e.end_time IS NOT NULL`, id).
		Scan(&userID, &project, &start, &end, &f.Description, &billable, &invoice, &number)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
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
	f.Invoice = invoiceRefOrNil(invoice, number)
	return f, userID == currentUser(r).ID, true
}

// changeable writes 403 or 409 itself unless the current user may change entry f.
func (a *App) changeable(w http.ResponseWriter, r *http.Request, f entryForm, mine bool) bool {
	switch {
	case !mine:
		a.forbidden(w, r)
	case f.Invoice != nil:
		a.errorPage(w, r, http.StatusConflict, "entries.error.claimed")
	default:
		return true
	}
	return false
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
	a.renderEntry(w, r, http.StatusOK, entryPage{Entry: f, ReadOnly: !mine || f.Invoice != nil}, f.Project)
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
	if !a.changeable(w, r, stored, mine) {
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
	s, errs, err := validateEntry(tx, currentUser(r), f, loc, stored.Project)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if errs != nil {
		a.renderEntry(w, r, http.StatusUnprocessableEntity, entryPage{Entry: f, Errors: errs}, stored.Project)
		return
	}
	res, err := tx.Exec(`UPDATE time_entry SET project_id = ?, start_time = ?, end_time = ?, description = ?, billable = ?
		WHERE id = ? AND invoice_line_id IS NULL`,
		s.project, s.start, s.end, f.Description, s.billable, f.ID)
	if !a.wroteUnclaimed(w, r, res, err) {
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	redirectBack(w, r, time.Unix(s.start, 0).In(loc).Format(dayLayout))
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
	if !a.changeable(w, r, f, mine) {
		return
	}
	res, err := a.db.Exec(`DELETE FROM time_entry WHERE id = ? AND invoice_line_id IS NULL`, f.ID)
	if !a.wroteUnclaimed(w, r, res, err) {
		return
	}
	redirectBack(w, r, f.Start[:len(dayLayout)])
}

// entryDeleteSelected deletes all selected entries, or none when any of them may not be deleted,
// and answers with the listing its query names.
func (a *App) entryDeleteSelected(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	if err := r.ParseForm(); err != nil {
		a.errorPage(w, r, http.StatusBadRequest, "error.invalid_page")
		return
	}
	selected := map[int64]bool{}
	var ids []any
	for _, s := range r.PostForm["id"] {
		id, err := strconv.ParseInt(s, 10, 64)
		if err != nil {
			a.notFound(w, r)
			return
		}
		selected[id] = true
		ids = append(ids, id)
	}
	if len(ids) == 0 {
		a.listEntries(w, r, http.StatusOK, nil, nil)
		return
	}
	in := " IN (?" + strings.Repeat(", ?", len(ids)-1) + ")"
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.Query(`SELECT e.user_id, u.display_name, e.start_time, e.end_time IS NULL, e.description, ci.id, ci.number
		FROM time_entry e JOIN user u ON u.id = e.user_id`+claimJoin+` WHERE e.id`+in+` ORDER BY e.start_time, e.id`, ids...)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	var blocked []entryRow
	for rows.Next() {
		var e entryRow
		var userID, start int64
		var invoice sql.NullInt64
		var number sql.NullString
		if err := rows.Scan(&userID, &e.User, &start, &e.Running, &e.Description, &invoice, &number); err != nil {
			a.serverError(w, r, err)
			return
		}
		e.Own, e.Start, e.Invoice = userID == u.ID, time.Unix(start, 0), invoiceRefOrNil(invoice, number)
		if !e.Selectable() {
			blocked = append(blocked, e)
		}
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if blocked != nil {
		a.listEntries(w, r, http.StatusUnprocessableEntity, selected, blocked)
		return
	}
	if _, err := tx.Exec(`DELETE FROM time_entry WHERE id`+in, ids...); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.listEntries(w, r, http.StatusOK, nil, nil)
}

// wroteUnclaimed reports whether a write guarded by "invoice_line_id IS NULL" changed the entry;
// otherwise it writes 500, or 409 when the entry was claimed after it was loaded.
func (a *App) wroteUnclaimed(w http.ResponseWriter, r *http.Request, res sql.Result, err error) bool {
	if err != nil {
		a.serverError(w, r, err)
		return false
	}
	n, err := res.RowsAffected()
	if err != nil {
		a.serverError(w, r, err)
		return false
	}
	if n == 0 {
		a.errorPage(w, r, http.StatusConflict, "entries.error.claimed")
		return false
	}
	return true
}

// redirectBack returns to the listing the entry was opened from, else to the given day.
func redirectBack(w http.ResponseWriter, r *http.Request, day string) {
	to := r.PostFormValue("return")
	if !strings.HasPrefix(to, "/entries?") {
		to = "/entries?day=" + day
	}
	http.Redirect(w, r, to, http.StatusSeeOther)
}
