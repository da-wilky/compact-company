package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /quotes/{id}/accept", a.authed(a.markOutcome("accepted_date")))
		mux.HandleFunc("POST /quotes/{id}/decline", a.authed(a.markOutcome("declined_date")))
		mux.HandleFunc("POST /quotes/{id}/undo", a.authed(a.undoOutcome))
	})
}

// markOutcome sets column, accepted_date or declined_date, on a sent quote that has neither.
func (a *App) markOutcome(column string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
		if err != nil {
			a.notFound(w, r)
			return
		}
		date := r.FormValue("date")
		if _, err := time.Parse(time.DateOnly, date); err != nil {
			a.refuseOutcome(w, r, id, date)
			return
		}
		res, err := a.db.Exec(`UPDATE quote SET `+column+` = ?
			WHERE id = ? AND number IS NOT NULL AND accepted_date IS NULL AND declined_date IS NULL`, date, id)
		a.afterWrite(w, r, res, err, quotePath(id))
	}
}

func (a *App) undoOutcome(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(`UPDATE quote SET accepted_date = NULL, declined_date = NULL WHERE id = ? AND number IS NOT NULL`, id)
	a.afterWrite(w, r, res, err, quotePath(id))
}

// refuseOutcome shows sent quote id with the submitted date and why it wasn't marked.
func (a *App) refuseOutcome(w http.ResponseWriter, r *http.Request, id int64, date string) {
	q, err := a.loadSentQuote(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	q.DateForm, q.Errors = date, []string{"quotes.error.outcome_date"}
	a.render(w, r, http.StatusUnprocessableEntity, "quote_sent", q)
}
