package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"testing"
)

var statusDelete = regexp.MustCompile(`action="/settings/statuses/(\d+)/delete" data-confirm="Delete status ([^"]*)\?"`)

// statusPath finds the status named name on the settings page.
func (c *client) statusPath(name string) string {
	c.t.Helper()
	for _, m := range statusDelete.FindAllStringSubmatch(c.get("/settings").body, -1) {
		if m[2] == name {
			return "/settings/statuses/" + m[1]
		}
	}
	c.t.Fatalf("no status %q on /settings", name)
	return ""
}

func statusForm(name string, done bool) url.Values {
	form := url.Values{"name": {name}}
	if done {
		form.Set("done", "on")
	}
	return form
}

// boardWithTodo creates a project holding one todo and returns the project's page.
func (c *client) boardWithTodo() string {
	c.t.Helper()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	return project
}

func TestAddedStatusBecomesTheLastBoardColumn(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	wantRedirect(t, c.postForm("/settings/statuses", statusForm("Archived", false)), "/settings#statuses")

	wantInOrder(t, c.get(project).text(), "Open", "In progress", "Waiting", "Done", "Archived")
}

func TestSettingsListsTheStatusesInOrder(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	page := c.get("/settings").in(t, "Statuses")

	wantInOrder(t, page.body, `value="Open"`, `value="In progress"`, `value="Waiting"`, `value="Done"`)
	page.wantBody(t, `<option value="2" selected>In progress</option>`)
}

func TestStatusWithoutNameIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	res := c.postForm("/settings/statuses", statusForm(" ", false))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Status name is required.")

	res = c.postForm(c.statusPath("Waiting"), statusForm("", false))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Status name is required.")

	wantInOrder(t, c.get(project).text(), "Open", "In progress", "Waiting", "Done")
}

func TestStatusIsRenamedAndMarkedDone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()
	waiting := c.statusPath("Waiting")

	wantRedirect(t, c.postForm(waiting, statusForm("Blocked", true)), "/settings#statuses")

	wantInOrder(t, c.get(project).text(), "Open", "In progress", "Blocked", "Done")
	c.get("/settings").wantBody(t, `value="Blocked"`)
	wantRedirect(t, c.postForm(c.statusPath("Done"), statusForm("Done", false)), "/settings#statuses")
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}

func TestDoneStatusMakesOverdueTodosNotOverdue(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	form := todoForm("Write copy")
	form.Set("due_date", "2026-10-01")
	c.createTodo(project, form)
	todo := c.todoPath(project, "Write copy")
	wantRedirect(t, c.postForm(todo+"/status", url.Values{"status": {"3"}}), project)
	c.get(project).wantBody(t, "Overdue")

	wantRedirect(t, c.postForm(c.statusPath("Waiting"), statusForm("Waiting", true)), "/settings#statuses")

	c.get(project).wantNoBody(t, "Overdue")
}

func TestStatusesAreReordered(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	wantRedirect(t, c.postForm(c.statusPath("Waiting")+"/up", nil), "/settings#statuses")
	wantInOrder(t, c.get(project).text(), "Open", "Waiting", "In progress", "Done")

	wantRedirect(t, c.postForm(c.statusPath("Open")+"/down", nil), "/settings#statuses")
	wantInOrder(t, c.get(project).text(), "Waiting", "Open", "In progress", "Done")
	wantInOrder(t, c.get("/settings").in(t, "Statuses").body, `value="Waiting"`, `value="Open"`, `value="In progress"`, `value="Done"`)

	wantRedirect(t, c.postForm(c.statusPath("Waiting")+"/up", nil), "/settings#statuses")
	wantRedirect(t, c.postForm(c.statusPath("Done")+"/down", nil), "/settings#statuses")
	wantInOrder(t, c.get(project).text(), "Waiting", "Open", "In progress", "Done")
}

func TestFirstStatusCannotBeDone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	res := c.postForm(c.statusPath("Open"), statusForm("Open", true))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The first status can&#39;t be a done status.")

	wantRedirect(t, c.postForm(c.statusPath("Done")+"/up", nil), "/settings#statuses")
	wantRedirect(t, c.postForm(c.statusPath("Done")+"/up", nil), "/settings#statuses")
	res = c.postForm(c.statusPath("Done")+"/up", nil)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The first status can&#39;t be a done status.")

	res = c.postForm(c.statusPath("Open")+"/down", nil)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The first status can&#39;t be a done status.")

	wantInOrder(t, c.get(project).text(), "Open", "Done", "In progress", "Waiting")
}

func TestDeletingTheFirstStatusMayNotPutADoneStatusFirst(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()
	todo := c.todoPath(project, "Write copy")
	wantRedirect(t, c.postForm(todo+"/status", url.Values{"status": {"3"}}), project)
	for range 2 {
		wantRedirect(t, c.postForm(c.statusPath("Done")+"/up", nil), "/settings#statuses")
	}

	res := c.postForm(c.statusPath("Open")+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The first status can&#39;t be a done status.")
	wantInOrder(t, c.get(project).text(), "Open", "Done", "In progress", "Waiting")
}

func TestUnusedStatusIsDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	wantRedirect(t, c.postForm(c.statusPath("Waiting")+"/delete", nil), "/settings#statuses")

	page := c.get(project)
	wantInOrder(t, page.text(), "Open", "In progress", "Done")
	page.wantNoBody(t, "Waiting")
}

func TestStatusATodoHasCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	res := c.postForm(c.statusPath("Open")+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Todos have this status. Move them to another status first.")
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}

func TestWorkingStatusCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.boardWithTodo()

	res := c.postForm(c.statusPath("In progress")+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This is the working status. Choose another one first.")
	wantInOrder(t, c.get(project).text(), "Open", "In progress", "Waiting", "Done")
}

func TestWorkingStatusIsChosenOrSetToNone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	wantRedirect(t, c.postForm("/settings/working-status", url.Values{"status": {"3"}}), "/settings#statuses")
	c.get("/settings").in(t, "Statuses").wantBody(t, `<option value="3" selected>Waiting</option>`)
	wantRedirect(t, c.postForm(c.statusPath("In progress")+"/delete", nil), "/settings#statuses")

	wantRedirect(t, c.postForm("/settings/working-status", url.Values{"status": {""}}), "/settings#statuses")
	c.get("/settings").in(t, "Statuses").wantBody(t, `<option value="" selected>None</option>`)
	wantRedirect(t, c.postForm(c.statusPath("Waiting")+"/delete", nil), "/settings#statuses")
}

func TestWorkingStatusCannotBeDone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.postForm("/settings/working-status", url.Values{"status": {"4"}})
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The working status can&#39;t be a done status.")

	res = c.postForm(c.statusPath("In progress"), statusForm("In progress", true))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The working status can&#39;t be a done status.")

	page := c.get("/settings").in(t, "Statuses")
	page.wantBody(t, `<option value="2" selected>In progress</option>`)
	c.postForm("/settings/working-status", url.Values{"status": {"99"}}).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestStatusesSpeakGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inProgress := c.statusPath("In progress")
	wantRedirect(t, c.postForm("/profile", url.Values{"language": {"de"}}), "/profile")

	res := c.postForm(inProgress+"/delete", nil)

	res.wantBody(t, "Das ist der Arbeitsstatus.")
	c.get("/settings").in(t, "Status").wantBody(t, "Arbeitsstatus")
}

func TestUnknownStatusIsNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	for _, path := range []string{"/settings/statuses/99", "/settings/statuses/99/up", "/settings/statuses/99/delete"} {
		c.postForm(path, statusForm("X", false)).wantStatus(t, http.StatusNotFound)
	}
}
