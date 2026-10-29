package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

const parallelConfig = devConfig + `
[defaults]
parallel_entries = true
`

// trackedTodo creates acme's projects and todo Write copy on website with two ended hours on it, the second not billable.
func (c *client) trackedTodo() (todo, website, hosting string) {
	c.t.Helper()
	website, hosting = c.acmeProjects()
	project := "/projects/" + website
	c.createTodo(project, todoForm("Write copy"))
	todo = c.todoPath(project, "Write copy")
	c.hxPost("/timers", startOn(todo)).wantStatus(c.t, http.StatusOK)
	c.hxPost("/timers", startOn(todo)).wantStatus(c.t, http.StatusOK)
	c.ta.clock.Advance(time.Hour)
	c.postForm("/timers/1/stop", nil).wantStatus(c.t, http.StatusSeeOther)
	c.postForm("/timers/2/stop", nil).wantStatus(c.t, http.StatusSeeOther)
	internal := entryForm(website, "2026-10-26T09:00", "2026-10-26T10:00", "internal")
	internal.Set("billable", "no")
	c.postForm("/entries/2", withTodo(internal, todo)).wantStatus(c.t, http.StatusSeeOther)
	c.get(project).wantBody(c.t, "2:00 h")
	return todo, website, hosting
}

func TestTodoPageOffersTheCustomersOtherActiveProjectsAsMoveTargets(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createProject(c.customerPath("acme"), projectForm("legacy", ""))
	legacy := c.projectPath(c.customerPath("acme"), "legacy")
	c.postForm(legacy+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("intranet", ""))
	c.createTodo("/projects/"+website, todoForm("Write copy"))

	page := c.get(c.todoPath("/projects/"+website, "Write copy"))

	_, move, _ := strings.Cut(page.body, "Move to another project")
	move, _, _ = strings.Cut(move, "</select>")
	if want := `<option value="` + hosting + `">hosting</option>`; strings.TrimSpace(move[strings.Index(move, "<option"):]) != want {
		t.Fatalf("move targets = %q, want %q", move, want)
	}
}

func TestTodoWithoutMoveTargetsOffersNoMove(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))

	c.get(c.todoPath(project, "Write copy")).wantNoBody(t, "Move to another project")
}

func TestMovingATodoMovesItsEntriesWithTheirTodoAndBillableFlags(t *testing.T) {
	ta := newTestApp(t, parallelConfig)
	c := ta.client()
	todo, website, hosting := c.trackedTodo()

	wantRedirect(t, c.postForm(todo+"/move", url.Values{"project": {hosting}}), "/projects/"+hosting)

	c.get("/projects/"+website).wantBody(t, "No todos yet.")
	board := c.get("/projects/" + hosting)
	wantCards(t, board.column(t, "In progress"), "Write copy")
	wantInOrder(t, board.column(t, "In progress").text(), "Write copy", "2:00 h")
	entries := c.get("/entries?day=2026-10-26")
	entries.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "Write copy", "", "yes")
	entries.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "Write copy", "internal", "no")
	entries.wantNoBody(t, "acme · website")
}

func TestMovingATodoOutsideTheCustomersActiveProjectsIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createProject(c.customerPath("acme"), projectForm("legacy", ""))
	legacy := c.projectPath(c.customerPath("acme"), "legacy")
	c.postForm(legacy+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("intranet", ""))
	intranet := c.projectPath(c.customerPath("globex"), "intranet")
	c.createTodo("/projects/"+website, todoForm("Write copy"))
	todo := c.todoPath("/projects/"+website, "Write copy")

	for _, target := range []string{intranet[len("/projects/"):], legacy[len("/projects/"):], website, "999", ""} {
		res := c.postForm(todo+"/move", url.Values{"project": {target}})
		res.wantStatus(t, http.StatusUnprocessableEntity)
		res.wantBody(t, "Choose another active project of the same customer.")
	}

	wantCards(t, c.get("/projects/"+website).column(t, "Open"), "Write copy")
}

func TestMovingATodoWithClaimedEntriesIsRefusedAndChangesNothing(t *testing.T) {
	ta := newTestApp(t, parallelConfig)
	c := ta.client()
	todo, website, hosting := c.trackedTodo()
	c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))

	res := c.postForm(todo+"/move", url.Values{"project": {hosting}})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This todo has time entries on a draft or issued invoice, so it can&#39;t move.")
	wantInOrder(t, c.get("/projects/"+website).text(), "Write copy", "2:00 h")
	c.get("/projects/"+hosting).wantBody(t, "No todos yet.")
	c.get("/entries?day=2026-10-26").wantNoBody(t, "acme · hosting")
}

func TestMovingATodoRendersInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createTodo("/projects/"+website, todoForm("Texte schreiben"))
	todo := c.todoPath("/projects/"+website, "Texte schreiben")
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	c.get(todo).wantBody(t, "In ein anderes Projekt verschieben")
	c.postForm(todo+"/move", url.Values{"project": {website}}).wantBody(t, "Wähle ein anderes aktives Projekt desselben Kunden.")
}
