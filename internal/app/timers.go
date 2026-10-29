package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /timers", a.authed(a.timerStart))
		mux.HandleFunc("POST /timers/{id}", a.authed(a.timerDescribe))
		mux.HandleFunc("POST /timers/{id}/stop", a.authed(a.timerStop))
	})
}

const overdueAfter = 12 * time.Hour

// timer is a running time entry; Project is "customer · project", "" when unassigned.
type timer struct {
	ID          int64
	Project     string
	Todo        string
	Billable    sql.NullBool
	Start       time.Time
	Description string
	Overdue     bool
}

type pickerProject struct {
	ID    int64
	Name  string
	Todos []pickerTodo
}

type pickerTodo struct {
	ID    int64
	Title string
}

type pickerCustomer struct {
	Name     string
	Projects []pickerProject
}

// timerBar is the header's timer controls; Error is a catalog key.
type timerBar struct {
	Running   []timer
	Customers []pickerCustomer
	Error     string
}

func (a *App) loadTimerBar(userID int64) (*timerBar, error) {
	bar := &timerBar{}
	rows, err := a.db.Query(`
		SELECT e.id, ifnull(c.name || ' · ' || p.name, ''), ifnull(t.title, ''), e.billable, e.start_time, e.description
		FROM time_entry e LEFT JOIN project p ON p.id = e.project_id LEFT JOIN customer c ON c.id = p.customer_id
			LEFT JOIN todo t ON t.id = e.todo_id
		WHERE e.user_id = ? AND e.end_time IS NULL ORDER BY e.start_time, e.id`, userID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	now := a.now()
	for rows.Next() {
		var t timer
		var start int64
		if err := rows.Scan(&t.ID, &t.Project, &t.Todo, &t.Billable, &start, &t.Description); err != nil {
			return nil, err
		}
		t.Start = time.Unix(start, 0)
		t.Overdue = now.Sub(t.Start) > overdueAfter
		bar.Running = append(bar.Running, t)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	bar.Customers, err = a.loadPicker(0, 0)
	return bar, err
}

// loadPicker lists the projects that may receive new time, plus storedProject, grouped by customer,
// each with its todos that may receive new time, plus storedTodo.
func (a *App) loadPicker(storedProject, storedTodo int64) ([]pickerCustomer, error) {
	rows, err := a.db.Query(`
		SELECT c.name, p.id, p.name, t.id, t.title FROM project p JOIN customer c ON c.id = p.customer_id
			LEFT JOIN todo t ON t.project_id = p.id AND ((p.archived = 0 AND c.archived = 0
				AND t.status_id IN (SELECT id FROM todo_status WHERE done = 0)) OR t.id = ?)
		WHERE (p.archived = 0 AND c.archived = 0) OR p.id = ?
		ORDER BY c.name, c.id, p.name, p.id, `+boardOrder, storedTodo, storedProject)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var customers []pickerCustomer
	for rows.Next() {
		var customer string
		var p pickerProject
		var todoID sql.NullInt64
		var todoTitle sql.NullString
		if err := rows.Scan(&customer, &p.ID, &p.Name, &todoID, &todoTitle); err != nil {
			return nil, err
		}
		if n := len(customers); n == 0 || customers[n-1].Name != customer {
			customers = append(customers, pickerCustomer{Name: customer})
		}
		last := &customers[len(customers)-1]
		if n := len(last.Projects); n == 0 || last.Projects[n-1].ID != p.ID {
			last.Projects = append(last.Projects, p)
		}
		if todoID.Valid {
			project := &last.Projects[len(last.Projects)-1]
			project.Todos = append(project.Todos, pickerTodo{ID: todoID.Int64, Title: todoTitle.String})
		}
	}
	return customers, rows.Err()
}

// entryConflicts reports whether an entry of u spanning [start, end) (end invalid = running)
// may not be saved because it overlaps another of u's entries, timers included.
// exclude is the entry being edited (0 for a new one).
func entryConflicts(tx *sql.Tx, u *User, exclude, start int64, end sql.NullInt64) (bool, error) {
	if u.ParallelEntries {
		return false, nil
	}
	var found bool
	err := tx.QueryRow(`SELECT EXISTS (SELECT 1 FROM time_entry
		WHERE user_id = ? AND id != ? AND (end_time IS NULL OR end_time > ?) AND (? IS NULL OR start_time < ?))`,
		u.ID, exclude, start, end, end).Scan(&found)
	return found, err
}

// activeProjectBillable returns the billable default of a project that may receive new time.
func activeProjectBillable(tx *sql.Tx, id int64) (bool, error) {
	var billable bool
	err := tx.QueryRow(`SELECT p.billable FROM project p JOIN customer c ON c.id = p.customer_id
		WHERE p.id = ? AND p.archived = 0 AND c.archived = 0`, id).Scan(&billable)
	return billable, err
}

// timerStart takes a project id, or todo:<id> for a todo, which implies its project.
func (a *App) timerStart(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	var projectID, todoID sql.NullInt64
	if s := r.PostFormValue("project"); s != "" {
		todo, isTodo := strings.CutPrefix(s, "todo:")
		if isTodo {
			s = todo
		}
		id, err := strconv.ParseInt(s, 10, 64)
		if err != nil {
			a.timerRefused(w, r, "timers.error.project")
			return
		}
		if isTodo {
			todoID = sql.NullInt64{Int64: id, Valid: true}
		} else {
			projectID = sql.NullInt64{Int64: id, Valid: true}
		}
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	if todoID.Valid {
		id, err := openTodoProject(tx, todoID.Int64)
		if errors.Is(err, sql.ErrNoRows) {
			a.timerRefused(w, r, "timers.error.todo")
			return
		}
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		projectID = sql.NullInt64{Int64: id, Valid: true}
	}
	var billable sql.NullBool
	if projectID.Valid {
		b, err := activeProjectBillable(tx, projectID.Int64)
		if errors.Is(err, sql.ErrNoRows) {
			a.timerRefused(w, r, "timers.error.project")
			return
		}
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		billable = sql.NullBool{Bool: b, Valid: true}
	}
	now := a.now().Unix()
	conflict, err := entryConflicts(tx, u, 0, now, sql.NullInt64{})
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if conflict {
		a.timerRefused(w, r, "timers.error.running")
		return
	}
	if _, err := tx.Exec(`INSERT INTO time_entry (user_id, project_id, todo_id, start_time, description, billable) VALUES (?, ?, ?, ?, ?, ?)`,
		u.ID, projectID, todoID, now, strings.TrimSpace(r.PostFormValue("description")), billable); err != nil {
		a.serverError(w, r, err)
		return
	}
	if todoID.Valid {
		if err := startWork(tx, todoID.Int64); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.timersChanged(w, r)
}

func (a *App) timerDescribe(w http.ResponseWriter, r *http.Request) {
	a.updateOwnTimer(w, r, `UPDATE time_entry SET description = ?`, strings.TrimSpace(r.PostFormValue("description")))
}

func (a *App) timerStop(w http.ResponseWriter, r *http.Request) {
	a.updateOwnTimer(w, r, `UPDATE time_entry SET end_time = ?`, a.now().Unix())
}

// updateOwnTimer applies set (with its argument) to the current user's running timer from the path, else 404.
func (a *App) updateOwnTimer(w http.ResponseWriter, r *http.Request, set string, arg any) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(set+` WHERE id = ? AND user_id = ? AND end_time IS NULL`, arg, id, currentUser(r).ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	n, err := res.RowsAffected()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if n == 0 {
		a.notFound(w, r)
		return
	}
	a.timersChanged(w, r)
}

func isHTMX(r *http.Request) bool {
	return r.Header.Get("HX-Request") == "true"
}

// timersChanged answers htmx with the refreshed header controls and plain forms with a redirect
// to the board they came from, else home.
func (a *App) timersChanged(w http.ResponseWriter, r *http.Request) {
	if isHTMX(r) {
		a.execute(w, r, http.StatusOK, "home", "timers", view{})
		return
	}
	http.Redirect(w, r, boardReturn(r.PostFormValue("return"), "/"), http.StatusSeeOther)
}

func (a *App) timerRefused(w http.ResponseWriter, r *http.Request, key string) {
	bar, err := a.loadTimerBar(currentUser(r).ID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	bar.Error = key
	name := "layout"
	if isHTMX(r) {
		name = "timers"
	}
	a.execute(w, r, http.StatusUnprocessableEntity, "home", name, view{Timers: bar})
}
