package app

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /projects/{id}/todos/new", a.authed(a.todoNew))
		mux.HandleFunc("POST /projects/{id}/todos", a.authed(a.todoCreate))
		mux.HandleFunc("GET /todos/{id}", a.authed(a.todoEdit))
		mux.HandleFunc("POST /todos/{id}", a.authed(a.todoUpdate))
		mux.HandleFunc("POST /todos/{id}/delete", a.authed(a.todoDelete))
		mux.HandleFunc("POST /todos/{id}/move", a.authed(a.todoMove))
		mux.HandleFunc("POST /todos/{id}/status", a.authed(a.todoSetStatus))
	})
}

// priorityKeys are the catalog keys of the stored priorities, indexed by value.
var priorityKeys = []string{"todos.priority.low", "todos.priority.normal", "todos.priority.high"}

const normalPriority = 1

type todoStatus struct {
	ID   int64
	Name string
	Done bool
}

type todoCard struct {
	ID       int64
	Title    string
	Project  string
	Assignee string
	Priority int
	DueDate  string
	Overdue  bool
	StatusID int64
	Tracked  time.Duration
	// Archived: the todo's project or customer is archived, which freezes its status.
	Archived  bool
	Startable bool
}

func (c todoCard) PriorityKey() string { return priorityKeys[c.Priority] }

func overdue(dueDate, today string) bool { return dueDate != "" && dueDate < today }

// boardOrder sorts todo t as every board shows it.
const boardOrder = `t.priority DESC, t.due_date IS NULL, t.due_date, t.created_at, t.id`

// firstStatus is the id of the status new todos get.
const firstStatus = `(SELECT id FROM todo_status ORDER BY position LIMIT 1)`

type boardColumn struct {
	Status todoStatus
	Todos  []todoCard
}

// board is every status as a column of the todos a page shows; Return is the page a status change goes back to.
// Cards name their project when the board spans projects.
type board struct {
	Columns      []boardColumn
	Return       string
	Empty        bool
	ShowProjects bool
}

// loadBoard loads the todos matching where (todo t, project p, customer c), sorted as every board shows them.
func (a *App) loadBoard(r *http.Request, ret, where string, args ...any) (board, error) {
	b := board{Return: ret, Empty: true}
	loc, err := a.viewerLocation(r)
	if err != nil {
		return b, err
	}
	today := a.now().In(loc).Format(time.DateOnly)
	statuses, err := a.db.Query(`SELECT id, name, done FROM todo_status ORDER BY position`)
	if err != nil {
		return b, err
	}
	defer func() { _ = statuses.Close() }()
	column := map[int64]int{}
	for statuses.Next() {
		var s todoStatus
		if err := statuses.Scan(&s.ID, &s.Name, &s.Done); err != nil {
			return b, err
		}
		column[s.ID] = len(b.Columns)
		b.Columns = append(b.Columns, boardColumn{Status: s})
	}
	if err := statuses.Err(); err != nil {
		return b, err
	}
	now := a.now().Unix()
	rows, err := a.db.Query(`SELECT t.id, t.title, p.name, ifnull(u.display_name, ''), t.priority, ifnull(t.due_date, ''), t.status_id, p.archived OR c.archived,
			(SELECT ifnull(sum(ifnull(e.end_time, ?) - e.start_time), 0) FROM time_entry e WHERE e.todo_id = t.id)
		FROM todo t JOIN project p ON p.id = t.project_id JOIN customer c ON c.id = p.customer_id LEFT JOIN user u ON u.id = t.assignee_id
		WHERE `+where+`
		ORDER BY `+boardOrder, append([]any{now}, args...)...)
	if err != nil {
		return b, err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var c todoCard
		var tracked int64
		if err := rows.Scan(&c.ID, &c.Title, &c.Project, &c.Assignee, &c.Priority, &c.DueDate, &c.StatusID, &c.Archived, &tracked); err != nil {
			return b, err
		}
		c.Tracked = (time.Duration(tracked) * time.Second).Truncate(time.Minute)
		col := &b.Columns[column[c.StatusID]]
		c.Overdue = !col.Status.Done && overdue(c.DueDate, today)
		c.Startable = !col.Status.Done && !c.Archived
		col.Todos = append(col.Todos, c)
		b.Empty = false
	}
	return b, rows.Err()
}

// openTodoProject returns the project of a todo that may receive new time: one not done.
// An entry on a todo always has this project; the migration's triggers enforce it.
func openTodoProject(tx *sql.Tx, id int64) (int64, error) {
	var project int64
	err := tx.QueryRow(`SELECT t.project_id FROM todo t JOIN todo_status s ON s.id = t.status_id WHERE t.id = ? AND s.done = 0`, id).Scan(&project)
	return project, err
}

// startWork moves a todo in the first status to the working status, if one is set.
func startWork(tx *sql.Tx, id int64) error {
	_, err := tx.Exec(`UPDATE todo SET status_id = (SELECT working_status_id FROM defaults)
		WHERE id = ? AND status_id = `+firstStatus+`
			AND (SELECT working_status_id FROM defaults) IS NOT NULL`, id)
	return err
}

func (a *App) projectBoard(r *http.Request, projectID int64) (board, error) {
	return a.loadBoard(r, "/projects/"+strconv.FormatInt(projectID, 10), `t.project_id = ?`, projectID)
}

// boardReturn is where a status change from a board goes back to: a board page, else fallback.
func boardReturn(to, fallback string) string {
	if strings.HasPrefix(to, "/projects/") || to == "/todos" || strings.HasPrefix(to, "/todos?") {
		return to
	}
	return fallback
}

type todoProject struct {
	ID       int64
	Name     string
	Archived bool
}

type todoUser struct {
	ID   int64
	Name string
}

type todo struct {
	ID          int64
	Title       string
	Description string
	AssigneeID  int64
	Priority    int
	DueDate     string
	Entries     int
}

type todoPage struct {
	Project    todoProject
	Todo       todo
	Users      []todoUser
	Priorities []string
	// MoveTargets are the projects the todo may move to.
	MoveTargets []todoProject
	Errors      []string
}

func (a *App) renderTodo(w http.ResponseWriter, r *http.Request, status int, page todoPage) {
	var err error
	if page.Users, err = a.todoUsers(); err != nil {
		a.serverError(w, r, err)
		return
	}
	if page.Todo.ID != 0 {
		if page.MoveTargets, err = a.moveTargets(page.Project.ID); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	page.Priorities = priorityKeys
	a.render(w, r, status, "todo", page)
}

// moveTarget selects (project id, name) of the other active projects of project ?1's customer.
const moveTarget = `SELECT t.id, t.name FROM project p JOIN project t ON t.customer_id = p.customer_id JOIN customer c ON c.id = t.customer_id
	WHERE p.id = ?1 AND t.id <> p.id AND NOT t.archived AND NOT c.archived`

func (a *App) moveTargets(projectID int64) ([]todoProject, error) {
	rows, err := a.db.Query(moveTarget+` ORDER BY t.name, t.id`, projectID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var targets []todoProject
	for rows.Next() {
		var p todoProject
		if err := rows.Scan(&p.ID, &p.Name); err != nil {
			return nil, err
		}
		targets = append(targets, p)
	}
	return targets, rows.Err()
}

func (a *App) todoUsers() ([]todoUser, error) {
	rows, err := a.db.Query(`SELECT id, display_name FROM user ORDER BY display_name, id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var users []todoUser
	for rows.Next() {
		var u todoUser
		if err := rows.Scan(&u.ID, &u.Name); err != nil {
			return nil, err
		}
		users = append(users, u)
	}
	return users, rows.Err()
}

// todoProjectFromPath writes 404 or 500 itself when ok is false.
func (a *App) todoProjectFromPath(w http.ResponseWriter, r *http.Request) (todoProject, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return todoProject{}, false
	}
	p := todoProject{ID: id}
	err = a.db.QueryRow(`SELECT p.name, p.archived OR c.archived FROM project p JOIN customer c ON c.id = p.customer_id WHERE p.id = ?`, id).
		Scan(&p.Name, &p.Archived)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return p, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return p, false
	}
	return p, true
}

func (a *App) todoNew(w http.ResponseWriter, r *http.Request) {
	p, ok := a.todoProjectFromPath(w, r)
	if !ok {
		return
	}
	page := todoPage{Project: p, Todo: todo{Priority: normalPriority}}
	if p.Archived {
		page.Errors = []string{"todos.error.archived"}
		a.renderTodo(w, r, http.StatusUnprocessableEntity, page)
		return
	}
	a.renderTodo(w, r, http.StatusOK, page)
}

// todoFromForm returns the submitted todo, its nullable columns and any validation errors as catalog keys.
func (a *App) todoFromForm(r *http.Request) (todo, sql.NullInt64, sql.NullString, []string, error) {
	t := todo{
		Title:       strings.TrimSpace(r.PostFormValue("title")),
		Description: strings.TrimSpace(r.PostFormValue("description")),
		DueDate:     r.PostFormValue("due_date"),
	}
	var errs []string
	if t.Title == "" {
		errs = append(errs, "todos.error.title")
	}
	var assignee sql.NullInt64
	if s := r.PostFormValue("assignee"); s != "" {
		id, err := strconv.ParseInt(s, 10, 64)
		var exists bool
		if err == nil {
			if err := a.db.QueryRow(`SELECT EXISTS (SELECT 1 FROM user WHERE id = ?)`, id).Scan(&exists); err != nil {
				return t, assignee, sql.NullString{}, nil, err
			}
		}
		if !exists {
			errs = append(errs, "todos.error.assignee")
		}
		t.AssigneeID, assignee = id, sql.NullInt64{Int64: id, Valid: exists}
	}
	p, err := strconv.Atoi(r.PostFormValue("priority"))
	if err != nil || p < 0 || p >= len(priorityKeys) {
		errs = append(errs, "todos.error.priority")
		p = normalPriority
	}
	t.Priority = p
	var due sql.NullString
	if t.DueDate != "" {
		_, err := time.Parse(time.DateOnly, t.DueDate)
		if err != nil {
			errs = append(errs, "todos.error.due_date")
		}
		due = sql.NullString{String: t.DueDate, Valid: err == nil}
	}
	return t, assignee, due, errs, nil
}

func (a *App) todoCreate(w http.ResponseWriter, r *http.Request) {
	p, ok := a.todoProjectFromPath(w, r)
	if !ok {
		return
	}
	t, assignee, due, errs, err := a.todoFromForm(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if p.Archived {
		errs = append([]string{"todos.error.archived"}, errs...)
	}
	if errs != nil {
		a.renderTodo(w, r, http.StatusUnprocessableEntity, todoPage{Project: p, Todo: t, Errors: errs})
		return
	}
	if _, err := a.db.Exec(`INSERT INTO todo (project_id, title, description, status_id, assignee_id, priority, due_date, created_at)
		VALUES (?, ?, ?, `+firstStatus+`, ?, ?, ?, ?)`,
		p.ID, t.Title, t.Description, assignee, t.Priority, due, a.now().Unix()); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/projects/"+strconv.FormatInt(p.ID, 10), http.StatusSeeOther)
}

// todoFromPath writes 404 or 500 itself when ok is false.
func (a *App) todoFromPath(w http.ResponseWriter, r *http.Request) (todoProject, todo, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return todoProject{}, todo{}, false
	}
	t := todo{ID: id}
	var p todoProject
	err = a.db.QueryRow(`SELECT p.id, p.name, p.archived OR c.archived, t.title, t.description, ifnull(t.assignee_id, 0), t.priority, ifnull(t.due_date, ''),
			(SELECT count(*) FROM time_entry WHERE todo_id = t.id)
		FROM todo t JOIN project p ON p.id = t.project_id JOIN customer c ON c.id = p.customer_id WHERE t.id = ?`, id).
		Scan(&p.ID, &p.Name, &p.Archived, &t.Title, &t.Description, &t.AssigneeID, &t.Priority, &t.DueDate, &t.Entries)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return p, t, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return p, t, false
	}
	return p, t, true
}

func (a *App) todoEdit(w http.ResponseWriter, r *http.Request) {
	p, t, ok := a.todoFromPath(w, r)
	if ok {
		a.renderTodo(w, r, http.StatusOK, todoPage{Project: p, Todo: t})
	}
}

func (a *App) todoUpdate(w http.ResponseWriter, r *http.Request) {
	p, stored, ok := a.todoFromPath(w, r)
	if !ok {
		return
	}
	t, assignee, due, errs, err := a.todoFromForm(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	t.ID, t.Entries = stored.ID, stored.Entries
	if errs != nil {
		a.renderTodo(w, r, http.StatusUnprocessableEntity, todoPage{Project: p, Todo: t, Errors: errs})
		return
	}
	if _, err := a.db.Exec(`UPDATE todo SET title = ?, description = ?, assignee_id = ?, priority = ?, due_date = ? WHERE id = ?`,
		t.Title, t.Description, assignee, t.Priority, due, t.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/projects/"+strconv.FormatInt(p.ID, 10), http.StatusSeeOther)
}

func (a *App) todoDelete(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	var projectID int64
	err = a.db.QueryRow(`DELETE FROM todo WHERE id = ? RETURNING project_id`, id).Scan(&projectID)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/projects/"+strconv.FormatInt(projectID, 10), http.StatusSeeOther)
}

// todoMove moves a todo and all its entries to another active project of the same customer, unless an entry is claimed.
func (a *App) todoMove(w http.ResponseWriter, r *http.Request) {
	p, t, ok := a.todoFromPath(w, r)
	if !ok {
		return
	}
	// An unparsable id is 0, which is no move target.
	target, _ := strconv.ParseInt(r.PostFormValue("project"), 10, 64)
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var allowed, claimed bool
	if err := tx.QueryRow(`SELECT EXISTS (`+moveTarget+` AND t.id = ?2)`, p.ID, target).Scan(&allowed); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.QueryRow(`SELECT EXISTS (SELECT 1 FROM time_entry WHERE todo_id = ? AND invoice_line_id IS NOT NULL)`, t.ID).Scan(&claimed); err != nil {
		a.serverError(w, r, err)
		return
	}
	var errs []string
	if !allowed {
		errs = append(errs, "todos.error.move_target")
	}
	if claimed {
		errs = append(errs, "todos.error.move_claimed")
	}
	if errs != nil {
		a.renderTodo(w, r, http.StatusUnprocessableEntity, todoPage{Project: p, Todo: t, Errors: errs})
		return
	}
	// The todo moves first: an entry whose project differs from its todo's loses the todo (0012's trigger).
	if _, err := tx.Exec(`UPDATE todo SET project_id = ? WHERE id = ?`, target, t.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if _, err := tx.Exec(`UPDATE time_entry SET project_id = ? WHERE todo_id = ?`, target, t.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/projects/"+strconv.FormatInt(target, 10), http.StatusSeeOther)
}

// todoSetStatus is the one way a todo's status changes, whichever board posts it.
func (a *App) todoSetStatus(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	status, err := strconv.ParseInt(r.PostFormValue("status"), 10, 64)
	var exists bool
	if err == nil {
		if err := a.db.QueryRow(`SELECT EXISTS (SELECT 1 FROM todo_status WHERE id = ?)`, status).Scan(&exists); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	if !exists {
		a.errorPage(w, r, http.StatusBadRequest, "todos.error.status")
		return
	}
	var projectID int64
	var archived bool
	err = a.db.QueryRow(`SELECT p.id, p.archived OR c.archived FROM todo t JOIN project p ON p.id = t.project_id JOIN customer c ON c.id = p.customer_id
		WHERE t.id = ?`, id).Scan(&projectID, &archived)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if archived {
		a.errorPage(w, r, http.StatusUnprocessableEntity, "todos.error.archived_status")
		return
	}
	if _, err := a.db.Exec(`UPDATE todo SET status_id = ? WHERE id = ?`, status, id); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, boardReturn(r.PostFormValue("return"), "/projects/"+strconv.FormatInt(projectID, 10)), http.StatusSeeOther)
}
