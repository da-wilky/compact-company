package app

import (
	"database/sql"
	"net/http"
	"slices"
	"strconv"
	"strings"
)

// selectionRefusal is what a refused mass action leaves on its list: the rows still selected,
// the catalog key saying why, and the rows that caused it (none when the action itself refused).
type selectionRefusal[T any] struct {
	Selected map[int64]bool
	Refusal  string
	Blocked  []T
}

// massAction is a write on the rows a list selects, applied to all of them or to none.
type massAction[T any] struct {
	// load returns one row for each of ids that exists, in the order a refusal names them.
	load   func(tx *sql.Tx, ids []int64) ([]T, error)
	blocks func(tx *sql.Tx, row *T) (bool, error)
	// blocked is the catalog key introducing the rows that block the action.
	blocked string
	// apply writes ids, or returns a catalog key refusing them.
	apply func(tx *sql.Tx, ids []int64) (string, error)
	// list renders the list the selection was made on.
	list func(status int, refused selectionRefusal[T])
}

// massWrite applies m to the ids r posts in one transaction, unless an id is unknown (404) or
// a row blocks it or apply refuses (422); either way it answers with m's list.
func massWrite[T any](a *App, w http.ResponseWriter, r *http.Request, m massAction[T]) {
	if err := r.ParseForm(); err != nil {
		a.errorPage(w, r, http.StatusBadRequest, "error.invalid_page")
		return
	}
	ids, ok := parseIDs(r.PostForm["id"])
	if !ok {
		a.notFound(w, r)
		return
	}
	if len(ids) == 0 {
		m.list(http.StatusOK, selectionRefusal[T]{})
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := m.load(tx, ids)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if len(rows) != len(ids) {
		a.notFound(w, r)
		return
	}
	var blocked []T
	for i := range rows {
		b, err := m.blocks(tx, &rows[i])
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		if b {
			blocked = append(blocked, rows[i])
		}
	}
	refusal := ""
	if blocked != nil {
		refusal = m.blocked
	} else if refusal, err = m.apply(tx, ids); err != nil {
		a.serverError(w, r, err)
		return
	}
	if refusal != "" {
		_ = tx.Rollback()
		selected := map[int64]bool{}
		for _, id := range ids {
			selected[id] = true
		}
		m.list(http.StatusUnprocessableEntity, selectionRefusal[T]{selected, refusal, blocked})
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	m.list(http.StatusOK, selectionRefusal[T]{})
}

// parseIDs parses ss as ids, sorted and without duplicates.
func parseIDs(ss []string) ([]int64, bool) {
	ids := make([]int64, len(ss))
	for i, s := range ss {
		id, err := strconv.ParseInt(s, 10, 64)
		if err != nil {
			return nil, false
		}
		ids[i] = id
	}
	slices.Sort(ids)
	return slices.Compact(ids), true
}

// inList is ids as an SQL " IN (?, …)" and its arguments.
func inList(ids []int64) (string, []any) {
	args := make([]any, len(ids))
	for i, id := range ids {
		args[i] = id
	}
	return " IN (?" + strings.Repeat(", ?", len(ids)-1) + ")", args
}
