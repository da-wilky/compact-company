package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"slices"
	"strings"
	"testing"
	"time"
)

func todoForm(title string) url.Values {
	return url.Values{"title": {title}, "description": {""}, "assignee": {""}, "priority": {"1"}, "due_date": {""}}
}

func (c *client) createTodo(projectPath string, form url.Values) {
	c.t.Helper()
	res := c.postForm(projectPath+"/todos", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != projectPath {
		c.t.Fatalf("Location = %q, want %s", loc, projectPath)
	}
}

var todoLink = regexp.MustCompile(`<a href="(/todos/\d+)">([^<]*)</a>`)

func (c *client) todoPath(projectPath, title string) string {
	c.t.Helper()
	for _, m := range todoLink.FindAllStringSubmatch(c.get(projectPath).body, -1) {
		if m[2] == title {
			return m[1]
		}
	}
	c.t.Fatalf("no todo %q on %s", title, projectPath)
	return ""
}

// column narrows a board page to the status column headed status, up to the next column.
func (r *response) column(t *testing.T, status string) *response {
	t.Helper()
	for _, part := range strings.Split(r.body, "<h3")[1:] {
		head, rest, ok := strings.Cut(part, "</h3>")
		if ok && textOf("<h3"+head) == status {
			return &response{Response: r.Response, body: rest}
		}
	}
	t.Fatalf("no column headed %q:\n%s", status, r.body)
	return nil
}

// wantCards checks the titles of the todos in r, in board order.
func wantCards(t *testing.T, r *response, want ...string) {
	t.Helper()
	var got []string
	for _, m := range todoLink.FindAllStringSubmatch(r.body, -1) {
		got = append(got, m[2])
	}
	if !slices.Equal(got, want) {
		t.Fatalf("cards = %q, want %q", got, want)
	}
}

// websitePath creates customer acme with project website and returns the project's page.
func (c *client) websitePath() string {
	c.t.Helper()
	return "/projects/" + c.websiteProject()
}

func TestNewTodoLandsInTheFirstStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()

	c.createTodo(project, todoForm("Write copy"))

	res := c.get(project)
	res.wantStatus(t, http.StatusOK)
	wantCards(t, res.column(t, "Open"), "Write copy")
	wantInOrder(t, res.text(), "Open", "In progress", "Waiting", "Done")
}

func TestProjectWithTodosCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))

	res := c.postForm(project+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project has todos.")
	c.get(project).wantStatus(t, http.StatusOK)
}

func TestTodoWithoutTitleIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	form := todoForm("  ")
	form.Set("description", "keep me")

	res := c.postForm(project+"/todos", form)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Title is required.")
	res.wantBody(t, "keep me")
	c.get(project).wantBody(t, "No todos yet.")
}

func TestTodoFieldsOutsideTheChoicesAreRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()

	for field, value := range map[string]string{"assignee": "999", "priority": "3", "due_date": "2026-13-01"} {
		form := todoForm("Write copy")
		form.Set(field, value)
		c.postForm(project+"/todos", form).wantStatus(t, http.StatusUnprocessableEntity)
	}
	c.get(project).wantBody(t, "No todos yet.")
}

func TestAnyUserEditsAnyTodo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	project := alice.websitePath()
	alice.createTodo(project, todoForm("Write copy"))
	todo := alice.todoPath(project, "Write copy")
	bobID := regexp.MustCompile(`<option value="(\d+)">bob</option>`).FindStringSubmatch(bob.get(todo).body)[1]

	form := url.Values{"title": {"Write landing copy"}, "description": {"hero and pricing"}, "assignee": {bobID}, "priority": {"2"}, "due_date": {"2026-10-30"}}
	res := bob.postForm(todo, form)

	res.wantStatus(t, http.StatusSeeOther)
	card := alice.get(project).column(t, "Open")
	wantCards(t, card, "Write landing copy")
	wantInOrder(t, card.text(), "Write landing copy", "High", "bob", "Due 2026-10-30")
	edit := alice.get(todo)
	edit.wantBody(t, "hero and pricing")
	edit.wantBody(t, `<option value="`+bobID+`" selected>bob</option>`)
	edit.wantBody(t, `<option value="2" selected>High</option>`)
	edit.wantBody(t, `value="2026-10-30"`)
}

func TestEditingATodoWithoutTitleIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")

	res := c.postForm(todo, todoForm(""))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Title is required.")
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}

func TestAnyUserDeletesAnyTodo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	project := alice.websitePath()
	alice.createTodo(project, todoForm("Write copy"))
	alice.createTodo(project, todoForm("Fix footer"))
	todo := alice.todoPath(project, "Write copy")
	wantConfirmation(t, bob.get(todo).confirmation(t, todo+"/delete"), "Delete todo Write copy?")

	res := bob.postForm(todo+"/delete", nil)

	wantRedirect(t, res, project)
	wantCards(t, alice.get(project).column(t, "Open"), "Fix footer")
	bob.get(todo).wantStatus(t, http.StatusNotFound)
}

func TestCardsAreSortedByPriorityThenDueDateThenAge(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	todo := func(title, priority, due string) {
		form := todoForm(title)
		form.Set("priority", priority)
		form.Set("due_date", due)
		c.createTodo(project, form)
		ta.clock.Advance(time.Minute)
	}
	todo("normal undated old", "1", "")
	todo("low soon", "0", "2026-10-27")
	todo("normal later", "1", "2026-11-05")
	todo("normal undated new", "1", "")
	todo("high undated", "2", "")
	todo("normal sooner", "1", "2026-10-28")

	wantCards(t, c.get(project).column(t, "Open"),
		"high undated", "normal sooner", "normal later", "normal undated old", "normal undated new", "low soon")
}

func TestOverdueBadgeOnlyOnNotDoneTodosPastTheirDueDateInTheUsersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	project := alice.websitePath()
	due := func(title, date string) string {
		form := todoForm(title)
		form.Set("due_date", date)
		alice.createTodo(project, form)
		return alice.todoPath(project, title)
	}
	due("yesterday", "2026-10-25")
	due("today", "2026-10-26")
	due("undated", "")
	done := due("done yesterday", "2026-10-25")
	alice.postForm(done+"/status", url.Values{"status": {statusID(t, alice.get(project), "Done")}}).wantStatus(t, http.StatusSeeOther)
	bob.postForm("/profile", url.Values{"language": {"en"}, "timezone": {"Pacific/Auckland"}}).wantStatus(t, http.StatusSeeOther)
	ta.clock.Advance(14 * time.Hour) // Monday 23:00 in UTC, Tuesday noon in Auckland

	open := alice.get(project).column(t, "Open")
	wantOverdue(t, open, "yesterday")
	wantOverdue(t, alice.get(project).column(t, "Done"))
	wantOverdue(t, bob.get(project).column(t, "Open"), "yesterday", "today")
}

// wantOverdue checks which cards in r carry the overdue badge.
func wantOverdue(t *testing.T, r *response, want ...string) {
	t.Helper()
	var got []string
	for _, card := range strings.Split(r.body, "<li")[1:] {
		if m := todoLink.FindStringSubmatch(card); m != nil && strings.Contains(textOf("<li"+card), "Overdue") {
			got = append(got, m[2])
		}
	}
	if !slices.Equal(got, want) {
		t.Fatalf("overdue cards = %q, want %q", got, want)
	}
}

// statusID reads a status's id from the first card's status dropdown on a board page.
func statusID(t *testing.T, board *response, name string) string {
	t.Helper()
	m := regexp.MustCompile(`<option value="(\d+)"[^>]*>` + regexp.QuoteMeta(name) + `</option>`).FindStringSubmatch(board.body)
	if m == nil {
		t.Fatalf("no status %q on the board:\n%s", name, board.body)
	}
	return m[1]
}

func TestStatusDropdownMovesTheCardWithoutJS(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	board := c.get(project)
	board.wantBody(t, `<input type="hidden" name="return" value="`+project+`">`)

	res := c.postForm(todo+"/status", url.Values{"status": {statusID(t, board, "Waiting")}, "return": {project}})

	wantRedirect(t, res, project)
	board = c.get(project)
	wantCards(t, board.column(t, "Open"))
	wantCards(t, board.column(t, "Waiting"), "Write copy")
}

func TestStatusDropdownUpdatesTheBoardInPlaceWithHtmx(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	// markup is the behaviour: htmx swaps the board the redirect lands on for the one on the page.
	c.get(project).wantBody(t, `hx-post="`+todo+`/status" hx-trigger="change" hx-target="#board" hx-select="#board"`)

	res := c.hxPost(todo+"/status", url.Values{"status": {statusID(t, c.get(project), "In progress")}, "return": {project}})

	wantRedirect(t, res, project)
	board := c.get(project)
	board.wantBody(t, `id="board"`)
	wantCards(t, board.column(t, "In progress"), "Write copy")
}

func TestStatusChangeRefusesUnknownStatusesTodosAndReturnPages(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	done := statusID(t, c.get(project), "Done")

	c.postForm(todo+"/status", url.Values{"status": {"999"}}).wantStatus(t, http.StatusBadRequest)
	c.postForm("/todos/999/status", url.Values{"status": {done}}).wantStatus(t, http.StatusNotFound)
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
	wantRedirect(t, c.postForm(todo+"/status", url.Values{"status": {done}, "return": {"https://evil.example/"}}), project)
}

func TestArchivedProjectTakesNoNewTodos(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	c.postForm(project+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	page := c.get(project)
	page.wantNoBody(t, `href="`+project+`/todos/new"`)
	c.get(project+"/todos/new").wantStatus(t, http.StatusUnprocessableEntity)
	res := c.postForm(project+"/todos", todoForm("Fix footer"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project is archived and takes no new todos.")
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}

func TestTodosRenderInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	form := todoForm("Texte schreiben")
	form.Set("priority", "2")
	form.Set("due_date", "2026-10-25")
	c.createTodo(project, form)
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	board := c.get(project)
	board.wantBody(t, "Aufgaben")
	board.wantBody(t, "Neue Aufgabe")
	wantInOrder(t, board.column(t, "Open").text(), "Texte schreiben", "Hoch", "Fällig 25.10.2026", "Überfällig")
	c.get(c.todoPath(project, "Texte schreiben")).wantBody(t, "Aufgabe bearbeiten")
}
