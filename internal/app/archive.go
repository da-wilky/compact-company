package app

import (
	"database/sql"
	"net/http"
	"strconv"
	"strings"
)

// guard refuses a write with the catalog key refusal while the EXISTS query holds for the id.
type guard struct {
	query, refusal string
}

// rowWrite runs stmts on one row's id unless one of its guards holds.
type rowWrite struct {
	guards []guard
	stmts  []string
}

var (
	customerArchive = rowWrite{
		[]guard{{`SELECT EXISTS (SELECT 1 FROM time_entry e JOIN project p ON p.id = e.project_id WHERE p.customer_id = ? AND e.end_time IS NULL)`,
			"customers.error.timer_running"}},
		[]string{`UPDATE customer SET archived = 1 WHERE id = ?`, `UPDATE project SET archived = 1 WHERE customer_id = ?`}}
	// Projects stay archived: some may have been archived on their own before.
	customerUnarchive = rowWrite{nil, []string{`UPDATE customer SET archived = 0 WHERE id = ?`}}
	// A customer's entries always belong to one of its projects, so no projects means no entries.
	customerDelete = rowWrite{
		[]guard{
			{`SELECT EXISTS (SELECT 1 FROM project WHERE customer_id = ?)`, "customers.error.has_projects"},
			{`SELECT EXISTS (SELECT 1 FROM invoice WHERE customer_id = ?)`, "customers.error.has_invoices"},
		},
		[]string{`DELETE FROM customer WHERE id = ?`}}
	projectArchive = rowWrite{
		[]guard{{`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ? AND end_time IS NULL)`, "projects.error.timer_running"}},
		[]string{`UPDATE project SET archived = 1 WHERE id = ?`}}
	projectUnarchive = rowWrite{nil, []string{`UPDATE project SET archived = 0 WHERE id = ?`}}
	projectDelete    = rowWrite{
		[]guard{
			{`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ?)`, "projects.error.has_entries"},
			{`SELECT EXISTS (SELECT 1 FROM invoice_line WHERE project_id = ?)`, "projects.error.on_invoice"},
		},
		[]string{`DELETE FROM project WHERE id = ?`}}
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /customers/{id}/archive", a.authed(a.customerWrite(customerArchive)))
		mux.HandleFunc("POST /customers/{id}/unarchive", a.authed(a.customerWrite(customerUnarchive)))
		mux.HandleFunc("POST /customers/{id}/delete", a.authed(a.customerWrite(customerDelete)))
		mux.HandleFunc("POST /customers/archive", a.authed(a.customersWrite(customerArchive, "customers.error.archive_blocked")))
		mux.HandleFunc("POST /customers/unarchive", a.authed(a.customersWrite(customerUnarchive, "")))
		mux.HandleFunc("POST /customers/delete", a.authed(a.customersWrite(customerDelete, "customers.error.delete_blocked")))
		mux.HandleFunc("POST /projects/{id}/archive", a.authed(a.projectWrite(projectArchive)))
		mux.HandleFunc("POST /projects/{id}/unarchive", a.authed(a.projectWrite(projectUnarchive)))
		mux.HandleFunc("POST /projects/{id}/delete", a.authed(a.projectWrite(projectDelete)))
		mux.HandleFunc("POST /customers/{id}/projects/archive", a.authed(a.projectsWrite(projectArchive, "projects.error.archive_blocked")))
		mux.HandleFunc("POST /customers/{id}/projects/unarchive", a.authed(a.projectsWrite(projectUnarchive, "")))
		mux.HandleFunc("POST /customers/{id}/projects/delete", a.authed(a.projectsWrite(projectDelete, "projects.error.delete_blocked")))
	})
}

func (a *App) customerWrite(rw rowWrite) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, ok := a.loadCustomer(w, r)
		if !ok {
			return
		}
		refuse := func(refusal string) { a.renderCustomer(w, r, http.StatusUnprocessableEntity, c, []string{refusal}) }
		a.guardedWrite(w, r, c.ID, rw, refuse, "/customers")
	}
}

func (a *App) projectWrite(rw rowWrite) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, p, ok := a.projectFromPath(w, r)
		if !ok {
			return
		}
		refuse := func(refusal string) {
			a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: []string{refusal}})
		}
		a.guardedWrite(w, r, p.ID, rw, refuse, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects")
	}
}

// guardedWrite runs rw on id in one transaction unless a guard holds for id,
// in which case it calls refuse with that guard's refusal instead.
func (a *App) guardedWrite(w http.ResponseWriter, r *http.Request, id int64, rw rowWrite, refuse func(string), done string) {
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	refusal, err := rw.refusal(tx, id)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if refusal != "" {
		refuse(refusal)
		return
	}
	if err := rw.run(tx, id); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, done, http.StatusSeeOther)
}

// refusal is the first guard holding for id, or "" when none does.
func (rw rowWrite) refusal(tx *sql.Tx, id int64) (string, error) {
	for _, g := range rw.guards {
		var blocked bool
		if err := tx.QueryRow(g.query, id).Scan(&blocked); err != nil {
			return "", err
		}
		if blocked {
			return g.refusal, nil
		}
	}
	return "", nil
}

func (rw rowWrite) run(tx *sql.Tx, id int64) error {
	for _, stmt := range rw.stmts {
		if _, err := tx.Exec(stmt, id); err != nil {
			return err
		}
	}
	return nil
}

// blockedRow names a selected row and the catalog key of what stops a mass write on it.
type blockedRow struct {
	Name, Reason string
}

// refusedSelection is what a refused mass write leaves on its list: the rows still selected,
// the catalog key introducing the refusal and the rows that caused it.
type refusedSelection struct {
	Selected map[int64]bool
	Refusal  string
	Blocked  []blockedRow
}

// listQuery keeps a list's archived filter across a mass write.
func listQuery(r *http.Request) string {
	if r.FormValue("archived") != "" {
		return "?archived=1"
	}
	return ""
}

func (a *App) customersWrite(rw rowWrite, refusal string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		refuse := func(s refusedSelection) { a.listCustomers(w, r, http.StatusUnprocessableEntity, s) }
		a.massWrite(w, r, `SELECT id, number || ' ' || name FROM customer WHERE id`, nil, rw, refusal, refuse, "/customers"+listQuery(r))
	}
}

func (a *App) projectsWrite(rw rowWrite, refusal string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, ok := a.customerFromPath(w, r)
		if !ok {
			return
		}
		refuse := func(s refusedSelection) { a.listProjects(w, r, http.StatusUnprocessableEntity, c, s) }
		a.massWrite(w, r, `SELECT id, name FROM project WHERE customer_id = ? AND id`, []any{c.ID}, rw, refusal, refuse,
			"/customers/"+strconv.FormatInt(c.ID, 10)+"/projects"+listQuery(r))
	}
}

// massWrite runs rw on every selected id in one transaction, or on none when a guard holds for any of them;
// then refuse gets every blocked row named by the names query, which must select (id, name) for the ids
// it is completed with. An id that query doesn't find is 404.
func (a *App) massWrite(w http.ResponseWriter, r *http.Request, names string, args []any, rw rowWrite, refusal string, refuse func(refusedSelection), done string) {
	if err := r.ParseForm(); err != nil {
		a.errorPage(w, r, http.StatusBadRequest, "error.invalid_page")
		return
	}
	selected := map[int64]bool{}
	for _, s := range r.PostForm["id"] {
		id, err := strconv.ParseInt(s, 10, 64)
		if err != nil {
			a.notFound(w, r)
			return
		}
		if !selected[id] {
			selected[id] = true
			args = append(args, id)
		}
	}
	if len(selected) == 0 {
		http.Redirect(w, r, done, http.StatusSeeOther)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.Query(names+` IN (?`+strings.Repeat(", ?", len(selected)-1)+`) ORDER BY id`, args...)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	var ids []int64
	var rowNames []string
	for rows.Next() {
		var id int64
		var name string
		if err := rows.Scan(&id, &name); err != nil {
			_ = rows.Close()
			a.serverError(w, r, err)
			return
		}
		ids = append(ids, id)
		rowNames = append(rowNames, name)
	}
	if err := rows.Close(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if len(ids) != len(selected) {
		a.notFound(w, r)
		return
	}
	var blocked []blockedRow
	for i, id := range ids {
		reason, err := rw.refusal(tx, id)
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		if reason != "" {
			blocked = append(blocked, blockedRow{rowNames[i], reason})
		}
	}
	if blocked != nil {
		_ = tx.Rollback()
		refuse(refusedSelection{Selected: selected, Refusal: refusal, Blocked: blocked})
		return
	}
	for _, id := range ids {
		if err := rw.run(tx, id); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, done, http.StatusSeeOther)
}
