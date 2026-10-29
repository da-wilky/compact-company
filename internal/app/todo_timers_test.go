package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

// startOn is what a card's start button posts for the todo at path.
func startOn(todo string) url.Values {
	return url.Values{"project": {"todo:" + strings.TrimPrefix(todo, "/todos/")}}
}

func TestCardStartButtonStartsATimerOnTheTodoAndItsProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.get(project).column(t, "Open").wantBody(t, `name="project" value="todo:`+strings.TrimPrefix(todo, "/todos/")+`"`)

	res := c.hxPost("/timers", startOn(todo))

	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.text(), "acme · website", "Write copy", "billable")
}

func TestCardStartWithoutJSReturnsToTheBoard(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	form := startOn(c.todoPath(project, "Write copy"))
	form.Set("return", project)

	wantRedirect(t, c.postForm("/timers", form), project)
	form.Set("return", "https://evil.example/")
	wantRedirect(t, c.postForm("/timers", form), "/")
}

func TestStartingATimerMovesATodoFromTheFirstStatusToTheWorkingStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))

	c.hxPost("/timers", startOn(c.todoPath(project, "Write copy"))).wantStatus(t, http.StatusOK)

	board := c.get(project)
	wantCards(t, board.column(t, "Open"))
	wantCards(t, board.column(t, "In progress"), "Write copy")
}

func TestStartingATimerLeavesATodoInTheFirstStatusWithoutAWorkingStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	wantRedirect(t, c.postForm("/settings/working-status", url.Values{"status": {""}}), "/settings#statuses")

	c.hxPost("/timers", startOn(c.todoPath(project, "Write copy"))).wantStatus(t, http.StatusOK)

	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}

func TestStartingATimerLeavesATodoInAnyOtherStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.postForm(todo+"/status", url.Values{"status": {statusID(t, c.get(project), "Waiting")}}).wantStatus(t, http.StatusSeeOther)

	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)

	board := c.get(project)
	wantCards(t, board.column(t, "Waiting"), "Write copy")
	wantCards(t, board.column(t, "In progress"))
}

func TestTimerPickerOffersNotDoneTodosOfActiveProjectsUnderTheirProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createTodo("/projects/"+website, todoForm("Write copy"))
	c.createTodo("/projects/"+website, todoForm("Old launch"))
	c.createTodo("/projects/"+hosting, todoForm("Renew domain"))
	old := c.todoPath("/projects/"+website, "Old launch")
	c.postForm(old+"/status", url.Values{"status": {statusID(t, c.get("/projects/"+website), "Done")}}).wantStatus(t, http.StatusSeeOther)
	c.createProject(c.customerPath("acme"), projectForm("legacy", ""))
	legacy := c.projectPath(c.customerPath("acme"), "legacy")
	c.createTodo(legacy, todoForm("Archive me"))
	c.postForm(legacy+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	writeCopy := strings.TrimPrefix(c.todoPath("/projects/"+website, "Write copy"), "/todos/")

	page := c.get("/customers")

	page.wantBody(t, `<option value="todo:`+writeCopy+`">website › Write copy</option>`)
	wantInOrder(t, page.text(), "hosting", "hosting › Renew domain", "website", "website › Write copy")
	page.wantNoBody(t, "Old launch")
	page.wantNoBody(t, "Archive me")
}

func TestMovingATodoToDoneLeavesItsRunningTimer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)

	c.postForm(todo+"/status", url.Values{"status": {statusID(t, c.get(project), "Done")}}).wantStatus(t, http.StatusSeeOther)

	// The picker leaves done todos out, so the title can only come from the running timer.
	page := c.get("/customers")
	page.wantBody(t, "Write copy")
	page.wantBody(t, "Stop")
}

func TestCardsShowTheHoursTrackedOnTheTodo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	c.createTodo(project, todoForm("Fix footer"))
	todo := c.todoPath(project, "Write copy")
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	ta.clock.Advance(90 * time.Minute)
	c.get(project).column(t, "In progress").wantBody(t, "1:30 h")

	c.postForm("/timers/1/stop", nil).wantStatus(t, http.StatusSeeOther)
	ta.clock.Advance(time.Hour)

	board := c.get(project)
	wantInOrder(t, board.column(t, "In progress").text(), "Write copy", "1:30 h")
	board.column(t, "Open").wantNoBody(t, "0:00 h")
}

func TestDeletingATodoKeepsItsEntriesOnTheProjectAndTheConfirmationCountsThem(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.get(todo).wantBody(t, `data-confirm="Delete todo Write copy?"`)
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	c.get(todo).wantBody(t, `data-confirm="Delete todo Write copy? Its time entry stays on the project without a todo."`)
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	ta.clock.Advance(90 * time.Minute)
	c.get(todo).wantBody(t, `data-confirm="Delete todo Write copy? Its 2 time entries stay on the project without a todo."`)

	wantRedirect(t, c.postForm(todo+"/delete", nil), project)

	c.get(todo).wantStatus(t, http.StatusNotFound)
	page := c.get("/customers")
	page.wantNoBody(t, "Write copy")
	page.wantBody(t, "acme · website")
}

func TestChangingAnEntrysProjectClearsItsTodo(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()
	website, hosting := c.acmeProjects()
	project := "/projects/" + website
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	ta.clock.Advance(time.Hour)
	c.postForm("/timers/1/stop", nil).wantStatus(t, http.StatusSeeOther)
	c.postForm("/timers/2/stop", nil).wantStatus(t, http.StatusSeeOther)
	c.get(project).wantBody(t, "2:00 h")

	c.postForm("/entries/1", withTodo(entryForm(website, "2026-10-26T09:00", "2026-10-26T10:00", "kept"), todo)).wantStatus(t, http.StatusSeeOther)
	c.get(project).wantBody(t, "2:00 h")
	c.postForm("/entries/1", withTodo(entryForm(hosting, "2026-10-26T09:00", "2026-10-26T10:00", "moved"), todo)).wantStatus(t, http.StatusSeeOther)
	c.massEdit("assign", "?day=2026-10-26", url.Values{"project": {hosting}}, "/entries/2").wantStatus(t, http.StatusOK)
	c.postForm("/entries/1", entryForm(website, "2026-10-26T09:00", "2026-10-26T10:00", "back")).wantStatus(t, http.StatusSeeOther)

	c.get(project).wantNoBody(t, ":00 h")
}

func TestTimeOnTodosRendersInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Texte schreiben"))
	todo := c.todoPath(project, "Texte schreiben")
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)
	c.get(project).wantBody(t, "Timer für Texte schreiben starten")

	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusOK)
	ta.clock.Advance(time.Hour)

	c.get(project).wantBody(t, "1:00 h")
	c.get(todo).wantBody(t, "Aufgabe Texte schreiben löschen? Ihr Zeiteintrag bleibt ohne Aufgabe im Projekt.")
	c.hxPost("/timers", url.Values{"project": {"todo:999"}}).wantBody(t, "Auf diese Aufgabe kann keine neue Zeit gebucht werden.")
}

func TestDoneTodosTakeNoNewTimers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.postForm(todo+"/status", url.Values{"status": {statusID(t, c.get(project), "Done")}}).wantStatus(t, http.StatusSeeOther)

	c.get(project).column(t, "Done").wantNoBody(t, "Start timer on Write copy")
	res := c.hxPost("/timers", startOn(todo))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This todo can&#39;t receive new time.")
	c.hxPost("/timers", url.Values{"project": {"todo:999"}}).wantStatus(t, http.StatusUnprocessableEntity)
	c.hxPost("/timers", url.Values{"project": {"todo:abc"}}).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestTodosOfArchivedProjectsTakeNoNewTimers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.get(project).wantBody(t, "Start timer on Write copy")
	c.postForm(project+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	c.get(project).wantNoBody(t, "Start timer on Write copy")
	c.hxPost("/timers", startOn(todo)).wantStatus(t, http.StatusUnprocessableEntity)
	wantCards(t, c.get(project).column(t, "Open"), "Write copy")
}
