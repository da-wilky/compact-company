package app

import (
	"net/http"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /customers/{id}/archive", a.authed(a.customerWrite(
			`SELECT EXISTS (SELECT 1 FROM time_entry e JOIN project p ON p.id = e.project_id WHERE p.customer_id = ? AND e.end_time IS NULL)`,
			"customers.error.timer_running",
			`UPDATE customer SET archived = 1 WHERE id = ?`,
			`UPDATE project SET archived = 1 WHERE customer_id = ?`)))
		// Projects stay archived: some may have been archived on their own before.
		mux.HandleFunc("POST /customers/{id}/unarchive", a.authed(a.customerWrite("", "",
			`UPDATE customer SET archived = 0 WHERE id = ?`)))
		// A customer's entries always belong to one of its projects, so no projects means no entries.
		mux.HandleFunc("POST /customers/{id}/delete", a.authed(a.customerWrite(
			`SELECT EXISTS (SELECT 1 FROM project WHERE customer_id = ?)`,
			"customers.error.has_projects",
			`DELETE FROM customer WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/archive", a.authed(a.projectWrite(
			`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ? AND end_time IS NULL)`,
			"projects.error.timer_running",
			`UPDATE project SET archived = 1 WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/unarchive", a.authed(a.projectWrite("", "",
			`UPDATE project SET archived = 0 WHERE id = ?`)))
		mux.HandleFunc("POST /projects/{id}/delete", a.authed(a.projectWrite(
			`SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ?)`,
			"projects.error.has_entries",
			`DELETE FROM project WHERE id = ?`)))
	})
}

func (a *App) customerWrite(guard, refusal string, stmts ...string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, ok := a.loadCustomer(w, r)
		if !ok {
			return
		}
		refuse := func() { a.renderCustomer(w, r, http.StatusUnprocessableEntity, c, []string{refusal}) }
		a.guardedWrite(w, r, c.ID, guard, refuse, "/customers", stmts)
	}
}

func (a *App) projectWrite(guard, refusal string, stmts ...string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, p, ok := a.projectFromPath(w, r)
		if !ok {
			return
		}
		refuse := func() {
			a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: []string{refusal}})
		}
		a.guardedWrite(w, r, p.ID, guard, refuse, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects", stmts)
	}
}

// guardedWrite runs stmts on id in one transaction unless the EXISTS query guard ("" = none) holds for id,
// in which case it calls refuse instead.
func (a *App) guardedWrite(w http.ResponseWriter, r *http.Request, id int64, guard string, refuse func(), done string, stmts []string) {
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	if guard != "" {
		var blocked bool
		if err := tx.QueryRow(guard, id).Scan(&blocked); err != nil {
			a.serverError(w, r, err)
			return
		}
		if blocked {
			refuse()
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
