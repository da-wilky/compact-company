package app

import (
	"net/http"
	"strconv"
)

// guard refuses a write with the catalog key refusal while the EXISTS query holds for the id.
type guard struct {
	query, refusal string
}

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /customers/{id}/archive", a.authed(a.customerWrite(
			[]guard{{`SELECT EXISTS (SELECT 1 FROM time_entry e JOIN project p ON p.id = e.project_id WHERE p.customer_id = ? AND e.end_time IS NULL)`,
				"customers.error.timer_running"}},
			`UPDATE customer SET archived = 1 WHERE id = ?`,
			`UPDATE project SET archived = 1 WHERE customer_id = ?`)))
		// Projects stay archived: some may have been archived on their own before.
		mux.HandleFunc("POST /customers/{id}/unarchive", a.authed(a.customerWrite(nil,
			`UPDATE customer SET archived = 0 WHERE id = ?`)))
		// A customer's entries always belong to one of its projects, so no projects means no entries.
		mux.HandleFunc("POST /customers/{id}/delete", a.authed(a.customerWrite(
			[]guard{
				{`SELECT EXISTS (SELECT 1 FROM project WHERE customer_id = ?)`, "customers.error.has_projects"},
				{`SELECT EXISTS (SELECT 1 FROM invoice WHERE customer_id = ?)`, "customers.error.has_invoices"},
			},
			`DELETE FROM customer WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/archive", a.authed(a.projectWrite(
			[]guard{{`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ? AND end_time IS NULL)`, "projects.error.timer_running"}},
			`UPDATE project SET archived = 1 WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/unarchive", a.authed(a.projectWrite(nil,
			`UPDATE project SET archived = 0 WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/delete", a.authed(a.projectWrite(
			[]guard{{`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ?)`, "projects.error.has_entries"}},
			`DELETE FROM project WHERE id = ?`)))
	})
}

func (a *App) customerWrite(guards []guard, stmts ...string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, ok := a.loadCustomer(w, r)
		if !ok {
			return
		}
		refuse := func(refusal string) { a.renderCustomer(w, r, http.StatusUnprocessableEntity, c, []string{refusal}) }
		a.guardedWrite(w, r, c.ID, guards, refuse, "/customers", stmts)
	}
}

func (a *App) projectWrite(guards []guard, stmts ...string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, p, ok := a.projectFromPath(w, r)
		if !ok {
			return
		}
		refuse := func(refusal string) {
			a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: []string{refusal}})
		}
		a.guardedWrite(w, r, p.ID, guards, refuse, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects", stmts)
	}
}

// guardedWrite runs stmts on id in one transaction unless a guard holds for id,
// in which case it calls refuse with that guard's refusal instead.
func (a *App) guardedWrite(w http.ResponseWriter, r *http.Request, id int64, guards []guard, refuse func(string), done string, stmts []string) {
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	for _, g := range guards {
		var blocked bool
		if err := tx.QueryRow(g.query, id).Scan(&blocked); err != nil {
			a.serverError(w, r, err)
			return
		}
		if blocked {
			refuse(g.refusal)
			return
		}
	}
	for _, stmt := range stmts {
		if _, err := tx.Exec(stmt, id); err != nil {
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
