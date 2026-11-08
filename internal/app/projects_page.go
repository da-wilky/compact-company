package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /projects", a.authed(a.projectListAll))
		mux.HandleFunc("GET /projects/new", a.authed(a.projectNewOnProjects))
		mux.HandleFunc("POST /projects", a.authed(a.projectCreateOnProjects))
	})
}

func (a *App) projectListAll(w http.ResponseWriter, r *http.Request) {
	a.listProjects(w, r, http.StatusOK, nil, selectionRefusal[blockedRow]{})
}

func (a *App) customerFilter(selected int64) ([]filterSelectOption, error) {
	customers, err := a.customerOptions()
	var options []filterSelectOption
	for _, c := range customers {
		options = append(options, filterSelectOption{Value: strconv.FormatInt(c.ID, 10), Name: c.Name, Archived: c.Archived, Selected: c.ID == selected})
	}
	return options, err
}

func (a *App) renderProjectOnProjects(w http.ResponseWriter, r *http.Request, status int, page projectPage) {
	page.OnProjects = true
	var err error
	if page.Customers, err = a.customerOptions(); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "project", page)
}

func (a *App) projectNewOnProjects(w http.ResponseWriter, r *http.Request) {
	customer, _ := strconv.ParseInt(r.FormValue("customer"), 10, 64)
	a.renderProjectOnProjects(w, r, http.StatusOK, projectPage{Customer: projectCustomer{ID: customer}, Project: project{Billable: true}})
}

func (a *App) projectCreateOnProjects(w http.ResponseWriter, r *http.Request) {
	id, _ := strconv.ParseInt(r.PostFormValue("customer"), 10, 64)
	c, err := a.loadProjectCustomer(id)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		a.serverError(w, r, err)
		return
	}
	refuse := func(p project, errs []string) {
		a.renderProjectOnProjects(w, r, http.StatusUnprocessableEntity, projectPage{Customer: c, Project: p, Errors: errs})
	}
	if err != nil {
		p, _, _ := projectFromForm(r, currency.XXX)
		refuse(p, []string{"projects.error.customer"})
		return
	}
	a.insertProject(w, r, c, refuse, "/projects")
}
