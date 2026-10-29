package app

import (
	"database/sql"
	"net/http"
	"strconv"
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
			{`SELECT EXISTS (SELECT 1 FROM todo WHERE project_id = ?)`, "projects.error.has_todos"},
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
			a.renderProject(w, r, http.StatusUnprocessableEntity, projectPage{Customer: c, Project: p, Errors: []string{refusal}})
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

// blockedRow is a selected customer or project as a refusal names it, with the catalog key of what blocks it.
type blockedRow struct {
	ID           int64
	Name, Reason string
}

// archiveActions are the URLs a customer or project list's selection posts to, keeping its archived filter,
// and the list's Active / All toggle.
type archiveActions struct {
	ArchiveURL, UnarchiveURL, DeleteURL string
	Shown                               []filterOption
}

// hidesArchived reports whether a list that hides archived rows is empty only because of that; archived selects them.
func (a *App) hidesArchived(show, empty bool, archived string, args ...any) (bool, error) {
	if show || !empty {
		return false, nil
	}
	var hides bool
	err := a.db.QueryRow(`SELECT EXISTS (`+archived+`)`, args...).Scan(&hides)
	return hides, err
}

func newArchiveActions(base string, showArchived bool) archiveActions {
	q := ""
	if showArchived {
		q = "?archived=1"
	}
	return archiveActions{base + "/archive" + q, base + "/unarchive" + q, base + "/delete" + q,
		[]filterOption{{"filter.active", base, !showArchived}, {"filter.all", base + "?archived=1", showArchived}}}
}

func (a *App) customersWrite(rw rowWrite, blocked string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		list := func(status int, s selectionRefusal[blockedRow]) { a.listCustomers(w, r, status, s) }
		a.rowsWrite(w, r, `SELECT id, number || ' ' || name FROM customer WHERE id`, nil, rw, blocked, list)
	}
}

func (a *App) projectsWrite(rw rowWrite, blocked string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c, ok := a.customerFromPath(w, r)
		if !ok {
			return
		}
		list := func(status int, s selectionRefusal[blockedRow]) { a.listProjects(w, r, status, c, s) }
		a.rowsWrite(w, r, `SELECT id, name FROM project WHERE customer_id = ? AND id`, []any{c.ID}, rw, blocked, list)
	}
}

// rowsWrite runs rw on every selected row; names selects (id, name) of the rows whose ids complete it, args first.
func (a *App) rowsWrite(w http.ResponseWriter, r *http.Request, names string, args []any, rw rowWrite, blocked string,
	list func(int, selectionRefusal[blockedRow])) {
	massWrite(a, w, r, massAction[blockedRow]{
		load: func(tx *sql.Tx, ids []int64) ([]blockedRow, error) {
			in, idArgs := inList(ids)
			rows, err := tx.Query(names+in+` ORDER BY id`, append(args, idArgs...)...)
			if err != nil {
				return nil, err
			}
			defer func() { _ = rows.Close() }()
			var found []blockedRow
			for rows.Next() {
				var b blockedRow
				if err := rows.Scan(&b.ID, &b.Name); err != nil {
					return nil, err
				}
				found = append(found, b)
			}
			return found, rows.Err()
		},
		blocks: func(tx *sql.Tx, b *blockedRow) (bool, error) {
			var err error
			b.Reason, err = rw.refusal(tx, b.ID)
			return b.Reason != "", err
		},
		blocked: blocked,
		apply: func(tx *sql.Tx, ids []int64) (string, error) {
			for _, id := range ids {
				if err := rw.run(tx, id); err != nil {
					return "", err
				}
			}
			return "", nil
		},
		list: list,
	})
}
