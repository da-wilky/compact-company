package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /settings/statuses", a.authed(a.statusCreate))
		mux.HandleFunc("POST /settings/statuses/{id}", a.authed(a.statusUpdate))
		mux.HandleFunc("POST /settings/statuses/{id}/up", a.authed(a.statusMove(-1)))
		mux.HandleFunc("POST /settings/statuses/{id}/down", a.authed(a.statusMove(+1)))
		mux.HandleFunc("POST /settings/statuses/{id}/delete", a.authed(a.statusDelete))
		mux.HandleFunc("POST /settings/working-status", a.authed(a.workingStatusSave))
	})
}

const statusesPath = "/settings#statuses"

type statusRow struct {
	todoStatus
	First, Last bool
}

type statusSettings struct {
	Statuses []statusRow
	Working  int64
}

func (a *App) loadStatusSettings() (statusSettings, error) {
	var s statusSettings
	if err := a.db.QueryRow(`SELECT ifnull(working_status_id, 0) FROM defaults`).Scan(&s.Working); err != nil {
		return s, err
	}
	rows, err := a.db.Query(`SELECT id, name, done FROM todo_status ORDER BY position`)
	if err != nil {
		return s, err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var st statusRow
		if err := rows.Scan(&st.ID, &st.Name, &st.Done); err != nil {
			return s, err
		}
		s.Statuses = append(s.Statuses, st)
	}
	if n := len(s.Statuses); n > 0 {
		s.Statuses[0].First, s.Statuses[n-1].Last = true, true
	}
	return s, rows.Err()
}

func (a *App) statusRefused(w http.ResponseWriter, r *http.Request, key string) {
	p, err := a.loadSettings()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	p.Error = key
	a.render(w, r, http.StatusUnprocessableEntity, "settings", p)
}

// errStatusRule carries the catalog key of the status rule a change breaks.
type errStatusRule string

func (e errStatusRule) Error() string { return string(e) }

// changeStatuses runs change in a transaction and commits it only if the first and the working status are still not done.
func (a *App) changeStatuses(w http.ResponseWriter, r *http.Request, change func(tx *sql.Tx) error) {
	err := func() error {
		tx, err := a.db.Begin()
		if err != nil {
			return err
		}
		defer func() { _ = tx.Rollback() }()
		if err := change(tx); err != nil {
			return err
		}
		var firstDone, workingDone bool
		err = tx.QueryRow(`SELECT done FROM todo_status WHERE id = ` + firstStatus).Scan(&firstDone)
		if errors.Is(err, sql.ErrNoRows) {
			return errStatusRule("settings.error.no_status")
		}
		if err != nil {
			return err
		}
		if err := tx.QueryRow(`SELECT EXISTS (SELECT 1 FROM defaults d JOIN todo_status s ON s.id = d.working_status_id WHERE s.done)`).
			Scan(&workingDone); err != nil {
			return err
		}
		switch {
		case firstDone:
			return errStatusRule("settings.error.first_done")
		case workingDone:
			return errStatusRule("settings.error.working_done")
		}
		return tx.Commit()
	}()
	var rule errStatusRule
	switch {
	case errors.As(err, &rule):
		a.statusRefused(w, r, string(rule))
	case errors.Is(err, sql.ErrNoRows):
		a.notFound(w, r)
	case err != nil:
		a.serverError(w, r, err)
	default:
		http.Redirect(w, r, statusesPath, http.StatusSeeOther)
	}
}

// statusFromForm returns the submitted name and done flag, or the rule an empty name breaks.
func statusFromForm(r *http.Request) (string, bool, error) {
	name := strings.TrimSpace(r.PostFormValue("name"))
	if name == "" {
		return "", false, errStatusRule("settings.error.status_name")
	}
	return name, r.PostFormValue("done") != "", nil
}

func (a *App) statusCreate(w http.ResponseWriter, r *http.Request) {
	a.changeStatuses(w, r, func(tx *sql.Tx) error {
		name, done, err := statusFromForm(r)
		if err != nil {
			return err
		}
		_, err = tx.Exec(`INSERT INTO todo_status (name, position, done) VALUES (?, (SELECT ifnull(max(position), 0) + 1 FROM todo_status), ?)`, name, done)
		return err
	})
}

// statusID reads the path's status id; an unparsable one is as unknown as a missing one.
func statusID(r *http.Request) int64 {
	id, _ := strconv.ParseInt(r.PathValue("id"), 10, 64)
	return id
}

func (a *App) statusUpdate(w http.ResponseWriter, r *http.Request) {
	a.changeStatuses(w, r, func(tx *sql.Tx) error {
		var id int64
		if err := tx.QueryRow(`SELECT id FROM todo_status WHERE id = ?`, statusID(r)).Scan(&id); err != nil {
			return err
		}
		name, done, err := statusFromForm(r)
		if err != nil {
			return err
		}
		_, err = tx.Exec(`UPDATE todo_status SET name = ?, done = ? WHERE id = ?`, name, done, id)
		return err
	})
}

// statusMove swaps the status with its neighbour in direction step (-1 up, +1 down); at the edge it does nothing.
func (a *App) statusMove(step int) http.HandlerFunc {
	neighbour := `SELECT id, position FROM todo_status WHERE position > ? ORDER BY position LIMIT 1`
	if step < 0 {
		neighbour = `SELECT id, position FROM todo_status WHERE position < ? ORDER BY position DESC LIMIT 1`
	}
	return func(w http.ResponseWriter, r *http.Request) {
		a.changeStatuses(w, r, func(tx *sql.Tx) error {
			id := statusID(r)
			var pos int64
			if err := tx.QueryRow(`SELECT position FROM todo_status WHERE id = ?`, id).Scan(&pos); err != nil {
				return err
			}
			var otherID, otherPos int64
			err := tx.QueryRow(neighbour, pos).Scan(&otherID, &otherPos)
			if errors.Is(err, sql.ErrNoRows) {
				return nil
			}
			if err != nil {
				return err
			}
			// position is UNIQUE, so the status steps aside while its neighbour takes its place.
			for _, set := range [][2]int64{{-pos, id}, {pos, otherID}, {otherPos, id}} {
				if _, err := tx.Exec(`UPDATE todo_status SET position = ? WHERE id = ?`, set[0], set[1]); err != nil {
					return err
				}
			}
			return nil
		})
	}
}

func (a *App) statusDelete(w http.ResponseWriter, r *http.Request) {
	a.changeStatuses(w, r, func(tx *sql.Tx) error {
		var id int64
		var used, working bool
		if err := tx.QueryRow(`SELECT id, EXISTS (SELECT 1 FROM todo WHERE status_id = s.id), EXISTS (SELECT 1 FROM defaults WHERE working_status_id = s.id)
			FROM todo_status s WHERE id = ?`, statusID(r)).Scan(&id, &used, &working); err != nil {
			return err
		}
		switch {
		case used:
			return errStatusRule("settings.error.status_used")
		case working:
			return errStatusRule("settings.error.status_working")
		}
		_, err := tx.Exec(`DELETE FROM todo_status WHERE id = ?`, id)
		return err
	})
}

func (a *App) workingStatusSave(w http.ResponseWriter, r *http.Request) {
	a.changeStatuses(w, r, func(tx *sql.Tx) error {
		var working sql.NullInt64
		if s := r.PostFormValue("status"); s != "" {
			id, _ := strconv.ParseInt(s, 10, 64)
			if err := tx.QueryRow(`SELECT id FROM todo_status WHERE id = ?`, id).Scan(&working.Int64); errors.Is(err, sql.ErrNoRows) {
				return errStatusRule("todos.error.status")
			} else if err != nil {
				return err
			}
			working.Valid = true
		}
		_, err := tx.Exec(`UPDATE defaults SET working_status_id = ?`, working)
		return err
	})
}
