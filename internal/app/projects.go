package app

import (
	"database/sql"
	"errors"
	"net/http"
	"net/url"
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
	CustomerID    int64
	Customer      string
	Currency      string
}

// projectsPage lists one customer's projects, or every project when Customer is nil.
type projectsPage struct {
	Customer *projectCustomer
	// Customers filters the list of every project.
	Customers    filterSelect
	CustomerID   int64
	NewURL       string
	Projects     []project
	ShowArchived bool
	// HidesArchived is set when the list is empty only because archived projects are hidden.
	HidesArchived bool
	selectionRefusal[blockedRow]
	archiveActions
}

type projectPage struct {
	Customer projectCustomer
	Project  project
	Errors   []string
	// OnProjects: a new project from the Projects page, which picks its customer from Customers.
	OnProjects bool
	Customers  []customerOption
	Board      board
	Quotes     quoteList
	// NewQuoteURL opens a new quote for the project.
	NewQuoteURL string
	// Quote is the accepted quote the project was created from, if any.
	Quote *quoteRef
}

func (a *App) renderProject(w http.ResponseWriter, r *http.Request, status int, page projectPage) {
	var quoteID sql.NullInt64
	var quoteNumber sql.NullString
	if err := a.db.QueryRow(`SELECT p.quote_id, q.number FROM project p LEFT JOIN quote q ON q.id = p.quote_id WHERE p.id = ?`, page.Project.ID).
		Scan(&quoteID, &quoteNumber); err != nil && !errors.Is(err, sql.ErrNoRows) {
		a.serverError(w, r, err)
		return
	}
	page.Quote = quoteRefOrNil(quoteID, quoteNumber)
	var err error
	if page.Board, err = a.loadBoard(r, "/projects/"+strconv.FormatInt(page.Project.ID, 10), `t.project_id = ?`, page.Project.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if page.Quotes.Rows, err = a.loadQuoteRows(`q.project_id = ?`, page.Project.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	page.NewQuoteURL = "/quotes/new?" + url.Values{"customer": {strconv.FormatInt(page.Customer.ID, 10)}, "project": {strconv.FormatInt(page.Project.ID, 10)}}.Encode()
	a.render(w, r, status, "project", page)
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
		a.listProjects(w, r, http.StatusOK, &c, selectionRefusal[blockedRow]{})
	}
}

func (a *App) listProjects(w http.ResponseWriter, r *http.Request, status int, c *projectCustomer, s selectionRefusal[blockedRow]) {
	show := r.FormValue("archived") != ""
	page := projectsPage{Customer: c, ShowArchived: show, selectionRefusal: s}
	var err error
	if c != nil {
		base := "/customers/" + strconv.FormatInt(c.ID, 10) + "/projects"
		page.CustomerID, page.NewURL, page.archiveActions = c.ID, base+"/new", newArchiveActions(base, nil, show)
	} else {
		if v := r.FormValue("customer"); v != "" {
			if page.CustomerID, err = strconv.ParseInt(v, 10, 64); err != nil {
				a.errorPage(w, r, http.StatusBadRequest, "error.invalid_filter")
				return
			}
		}
		q := url.Values{}
		page.NewURL = "/projects/new"
		if page.CustomerID != 0 {
			q.Set("customer", strconv.FormatInt(page.CustomerID, 10))
			page.NewURL += "?" + q.Encode()
		}
		page.archiveActions = newArchiveActions("/projects", q, show)
		page.Customers = filterSelect{Action: "/projects", Name: "customer", Label: "projects.customer", All: "projects.all_customers"}
		if show {
			page.Customers.Hidden = []hiddenField{{"archived", "1"}}
		}
		page.Customers.Groups = []filterGroup{{}}
		if page.Customers.Groups[0].Options, err = a.customerFilter(page.CustomerID); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	rows, err := a.db.Query(`SELECT p.id, p.name, p.hourly_rate, p.billable, p.archived, c.id, c.name, c.currency, c.hourly_rate
		FROM project p JOIN customer c ON c.id = p.customer_id
		WHERE ? IN (0, p.customer_id) AND (? OR p.archived = 0) ORDER BY c.name, c.id, p.id`, page.CustomerID, page.ShowArchived)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var p project
		var rate sql.NullInt64
		var customerRate int64
		if err := rows.Scan(&p.ID, &p.Name, &rate, &p.Billable, &p.Archived, &p.CustomerID, &p.Customer, &p.Currency, &customerRate); err != nil {
			a.serverError(w, r, err)
			return
		}
		unit := currency.MustParseISO(p.Currency)
		p.EffectiveRate = formatAmount(customerRate, unit)
		if rate.Valid {
			p.HourlyRate = formatAmount(rate.Int64, unit)
			p.EffectiveRate = p.HourlyRate
		}
		page.Projects = append(page.Projects, p)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if page.HidesArchived, err = a.hidesArchived(show, page.Projects == nil, `SELECT 1 FROM project WHERE ? IN (0, customer_id) AND archived = 1`, page.CustomerID); err != nil {
		a.serverError(w, r, err)
		return
	}
	name := "projects"
	if c != nil {
		name = "customer_projects"
	}
	a.render(w, r, status, name, page)
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
	refuse := func(p project, errs []string) {
		a.render(w, r, http.StatusUnprocessableEntity, "project", projectPage{Customer: c, Project: p, Errors: errs})
	}
	a.insertProject(w, r, c, refuse, "/customers/"+strconv.FormatInt(c.ID, 10)+"/projects")
}

// insertProject creates the submitted project of c and redirects to done, or calls refuse with the form and its errors.
func (a *App) insertProject(w http.ResponseWriter, r *http.Request, c projectCustomer, refuse func(project, []string), done string) {
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var cur string
	var archived bool
	if err := tx.QueryRow(`SELECT currency, archived FROM customer WHERE id = ?`, c.ID).Scan(&cur, &archived); err != nil {
		a.serverError(w, r, err)
		return
	}
	c.unit = currency.MustParseISO(cur)
	p, rate, errs := projectFromForm(r, c.unit)
	if archived {
		errs = []string{"projects.error.customer_archived"}
	}
	if errs != nil {
		_ = tx.Rollback()
		refuse(p, errs)
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
	http.Redirect(w, r, done, http.StatusSeeOther)
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
	a.renderProject(w, r, http.StatusOK, projectPage{Customer: c, Project: p})
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
		a.renderProject(w, r, http.StatusUnprocessableEntity, projectPage{Customer: c, Project: p, Errors: errs})
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
