package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /customers/{id}/projects", a.authed(a.projectList))
		mux.HandleFunc("GET /customers/{id}/projects/new", a.authed(a.projectNew))
		mux.HandleFunc("POST /customers/{id}/projects", a.authed(a.projectCreate))
		mux.HandleFunc("GET /projects/{id}", a.authed(a.projectEdit))
		mux.HandleFunc("POST /projects/{id}", a.authed(a.projectUpdate))
	})
}

// projectCustomer is the owning customer as project pages show it; HourlyRate is formatted.
type projectCustomer struct {
	ID         int64
	Number     string
	Name       string
	Currency   string
	HourlyRate string
	unit       currency.Unit
}

// project holds the override as the decimal the user sees ("" = customer's rate).
type project struct {
	ID            int64
	Name          string
	HourlyRate    string
	EffectiveRate string
	Billable      bool
	Archived      bool
}

type projectsPage struct {
	Customer     projectCustomer
	Projects     []project
	ShowArchived bool
	refusedSelection
}

type projectPage struct {
	Customer projectCustomer
	Project  project
	Errors   []string
}

func (a *App) loadProjectCustomer(id int64) (projectCustomer, error) {
	c := projectCustomer{ID: id}
	var rate int64
	err := a.db.QueryRow(`SELECT number, name, currency, hourly_rate FROM customer WHERE id = ?`, id).
		Scan(&c.Number, &c.Name, &c.Currency, &rate)
	if err != nil {
		return c, err
	}
	c.unit = currency.MustParseISO(c.Currency)
	c.HourlyRate = formatAmount(rate, c.unit)
	return c, nil
}

// customerUnit is read inside the write transaction so a concurrent currency change can't mis-scale the rate.
func customerUnit(tx *sql.Tx, id int64) (currency.Unit, error) {
	var cur string
	if err := tx.QueryRow(`SELECT currency FROM customer WHERE id = ?`, id).Scan(&cur); err != nil {
		return currency.Unit{}, err
	}
	return currency.MustParseISO(cur), nil
}

// customerFromPath writes 404 or 500 itself when ok is false.
func (a *App) customerFromPath(w http.ResponseWriter, r *http.Request) (projectCustomer, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return projectCustomer{}, false
	}
	c, err := a.loadProjectCustomer(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return c, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return c, false
	}
	return c, true
}

func (a *App) projectList(w http.ResponseWriter, r *http.Request) {
	c, ok := a.customerFromPath(w, r)
	if ok {
		a.listProjects(w, r, http.StatusOK, c, refusedSelection{})
	}
}

func (a *App) listProjects(w http.ResponseWriter, r *http.Request, status int, c projectCustomer, s refusedSelection) {
	page := projectsPage{Customer: c, ShowArchived: r.FormValue("archived") != "", refusedSelection: s}
	rows, err := a.db.Query(`SELECT id, name, hourly_rate, billable, archived FROM project
		WHERE customer_id = ? AND (? OR archived = 0) ORDER BY id`, c.ID, page.ShowArchived)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var p project
		var rate sql.NullInt64
		if err := rows.Scan(&p.ID, &p.Name, &rate, &p.Billable, &p.Archived); err != nil {
			a.serverError(w, r, err)
			return
		}
		p.EffectiveRate = c.HourlyRate
		if rate.Valid {
			p.HourlyRate = formatAmount(rate.Int64, c.unit)
			p.EffectiveRate = p.HourlyRate
		}
		page.Projects = append(page.Projects, p)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "projects", page)
}

func (a *App) projectNew(w http.ResponseWriter, r *http.Request) {
	c, ok := a.customerFromPath(w, r)
	if !ok {
		return
	}
	a.render(w, r, http.StatusOK, "project", projectPage{Customer: c, Project: project{Billable: true}})
}

// projectFromForm returns the submitted project, its override in minor units
// (invalid = none) and any validation errors as catalog keys.
func projectFromForm(r *http.Request, unit currency.Unit) (project, sql.NullInt64, []string) {
	p := project{
		Name:       strings.TrimSpace(r.PostFormValue("name")),
		HourlyRate: strings.TrimSpace(r.PostFormValue("hourly_rate")),
		Billable:   r.PostFormValue("billable") != "",
	}
	var errs []string
	if p.Name == "" {
		errs = append(errs, "projects.error.name")
	}
	var rate sql.NullInt64
	if p.HourlyRate != "" {
		v, err := parseAmount(p.HourlyRate, unit)
		if err != nil {
			errs = append(errs, "projects.error.hourly_rate")
		}
		rate = sql.NullInt64{Int64: v, Valid: err == nil}
	}
	return p, rate, errs
}

func (a *App) projectCreate(w http.ResponseWriter, r *http.Request) {
	c, ok := a.customerFromPath(w, r)
	if !ok {
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	if c.unit, err = customerUnit(tx, c.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	p, rate, errs := projectFromForm(r, c.unit)
	if errs != nil {
		_ = tx.Rollback()
		a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: errs})
		return
	}
	if _, err := tx.Exec(`INSERT INTO project (customer_id, name, hourly_rate, billable) VALUES (?, ?, ?, ?)`,
		c.ID, p.Name, rate, p.Billable); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects", http.StatusSeeOther)
}

// projectFromPath writes 404 or 500 itself when ok is false.
func (a *App) projectFromPath(w http.ResponseWriter, r *http.Request) (projectCustomer, project, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return projectCustomer{}, project{}, false
	}
	p := project{ID: id}
	var customerID int64
	var rate sql.NullInt64
	err = a.db.QueryRow(`SELECT customer_id, name, hourly_rate, billable, archived FROM project WHERE id = ?`, id).
		Scan(&customerID, &p.Name, &rate, &p.Billable, &p.Archived)
	var c projectCustomer
	if err == nil {
		c, err = a.loadProjectCustomer(customerID)
	}
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return c, p, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return c, p, false
	}
	if rate.Valid {
		p.HourlyRate = formatAmount(rate.Int64, c.unit)
	}
	return c, p, true
}

func (a *App) projectEdit(w http.ResponseWriter, r *http.Request) {
	c, p, ok := a.projectFromPath(w, r)
	if !ok {
		return
	}
	a.render(w, r, http.StatusOK, "project", projectPage{Customer: c, Project: p})
}

func (a *App) projectUpdate(w http.ResponseWriter, r *http.Request) {
	c, stored, ok := a.projectFromPath(w, r)
	if !ok {
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	if c.unit, err = customerUnit(tx, c.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	p, rate, errs := projectFromForm(r, c.unit)
	p.ID, p.Archived = stored.ID, stored.Archived
	if errs != nil {
		_ = tx.Rollback()
		a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: errs})
		return
	}
	if _, err := tx.Exec(`UPDATE project SET name = ?, hourly_rate = ?, billable = ? WHERE id = ?`,
		p.Name, rate, p.Billable, p.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects", http.StatusSeeOther)
}
