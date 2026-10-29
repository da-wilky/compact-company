package app

import (
	"net/http"
	"net/url"
	"strconv"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /todos", a.authed(a.todoBoard))
	})
}

// todosFilter is the Todos page's filter row; Assignee is "", "me" or a user id, so a link to "me" works for whoever opens it.
type todosFilter struct {
	Customer, Project, Status int64
	Assignee                  string
}

func (f todosFilter) query() url.Values {
	q := url.Values{}
	for name, id := range map[string]int64{"customer": f.Customer, "project": f.Project, "status": f.Status} {
		if id != 0 {
			q.Set(name, strconv.FormatInt(id, 10))
		}
	}
	if f.Assignee != "" {
		q.Set("assignee", f.Assignee)
	}
	return q
}

func (f todosFilter) url() string {
	if q := f.query(); len(q) > 0 {
		return "/todos?" + q.Encode()
	}
	return "/todos"
}

func (f todosFilter) selectFor(name, label, all string) filterSelect {
	return filterSelect{Action: "/todos", Name: name, Label: label, All: all, Hidden: hiddenFields(f.query(), name)}
}

type todosPage struct {
	Board                                    board
	Filtered                                 bool
	Customers, Projects, Assignees, Statuses filterSelect
}

func (a *App) todoBoard(w http.ResponseWriter, r *http.Request) {
	var f todosFilter
	ids := map[string]*int64{"customer": &f.Customer, "project": &f.Project, "status": &f.Status}
	for name, id := range ids {
		if s := r.FormValue(name); s != "" {
			var err error
			if *id, err = strconv.ParseInt(s, 10, 64); err != nil {
				a.errorPage(w, r, http.StatusBadRequest, "error.invalid_filter")
				return
			}
		}
	}
	assignee := int64(0)
	switch f.Assignee = r.FormValue("assignee"); f.Assignee {
	case "":
	case "me":
		assignee = currentUser(r).ID
	default:
		var err error
		if assignee, err = strconv.ParseInt(f.Assignee, 10, 64); err != nil {
			a.errorPage(w, r, http.StatusBadRequest, "error.invalid_filter")
			return
		}
	}
	b, err := a.loadBoard(r, f.url(), `NOT p.archived AND NOT c.archived
		AND (? IN (0, c.id)) AND (? IN (0, p.id)) AND (? IN (0, t.assignee_id)) AND (? IN (0, t.status_id))`,
		f.Customer, f.Project, assignee, f.Status)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	b.ShowProjects = true
	page := todosPage{Board: b, Filtered: len(f.query()) > 0}
	if err := a.todoFilters(&page, f); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "todos", page)
}

// todoFilters offers the active customers and projects, every user and every status.
func (a *App) todoFilters(page *todosPage, f todosFilter) error {
	page.Customers = f.selectFor("customer", "todos.customer", "todos.all_customers")
	page.Projects = f.selectFor("project", "todos.project", "todos.all_projects")
	page.Assignees = f.selectFor("assignee", "todos.assignee", "todos.everyone")
	page.Statuses = f.selectFor("status", "todos.status", "todos.all_statuses")

	customers := filterGroup{}
	rows, err := a.db.Query(`SELECT c.id, c.name, p.id, p.name FROM project p JOIN customer c ON c.id = p.customer_id
		WHERE NOT p.archived AND NOT c.archived ORDER BY c.name, c.id, p.name, p.id`)
	if err != nil {
		return err
	}
	defer func() { _ = rows.Close() }()
	var lastCustomer int64
	for rows.Next() {
		var customer, project int64
		var customerName, projectName string
		if err := rows.Scan(&customer, &customerName, &project, &projectName); err != nil {
			return err
		}
		if customer != lastCustomer {
			customers.Options = append(customers.Options, filterSelectOption{Value: strconv.FormatInt(customer, 10), Name: customerName, Selected: customer == f.Customer})
			page.Projects.Groups = append(page.Projects.Groups, filterGroup{Label: customerName})
			lastCustomer = customer
		}
		last := &page.Projects.Groups[len(page.Projects.Groups)-1]
		last.Options = append(last.Options, filterSelectOption{Value: strconv.FormatInt(project, 10), Name: projectName, Selected: project == f.Project})
	}
	if err := rows.Err(); err != nil {
		return err
	}
	page.Customers.Groups = []filterGroup{customers}

	users, err := a.todoUsers()
	if err != nil {
		return err
	}
	assignees := filterGroup{Options: []filterSelectOption{{Value: "me", Key: "todos.me", Selected: f.Assignee == "me"}}}
	for _, u := range users {
		id := strconv.FormatInt(u.ID, 10)
		assignees.Options = append(assignees.Options, filterSelectOption{Value: id, Name: u.Name, Selected: id == f.Assignee})
	}
	page.Assignees.Groups = []filterGroup{assignees}

	statuses := filterGroup{}
	for _, col := range page.Board.Columns {
		statuses.Options = append(statuses.Options, filterSelectOption{Value: strconv.FormatInt(col.Status.ID, 10), Name: col.Status.Name, Selected: col.Status.ID == f.Status})
	}
	page.Statuses.Groups = []filterGroup{statuses}
	return nil
}
