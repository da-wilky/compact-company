package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
)

func todoID(path string) string {
	return strings.TrimPrefix(path, "/todos/")
}

// withTodo is form with its todo set to the todo at path ("" = none).
func withTodo(form url.Values, path string) url.Values {
	form.Set("todo", todoID(path))
	return form
}

func TestPickingATodoOnAnEntrySetsItsProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createTodo("/projects/"+hosting, todoForm("Migrate DNS"))
	todo := c.todoPath("/projects/"+hosting, "Migrate DNS")
	entry := c.createEntry(entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "dns work"))

	res := c.postForm(entry, withTodo(entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "dns work"), todo))

	wantRedirect(t, res, "/entries?day=2026-10-26")
	c.get("/entries?day=2026-10-26").wantRow(t, "", "08:00–09:00", "1:00", "acme · hosting", "Migrate DNS", "dns work", "yes")
	edit := c.get(entry)
	edit.wantBody(t, `<option value="`+hosting+`" selected>hosting</option>`)
	edit.wantBody(t, `<optgroup label="acme · hosting"><option value="`+todoID(todo)+`" selected>Migrate DNS</option>`)
}

func TestPickingAnotherProjectOrNoTodoClearsTheEntrysTodo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createTodo("/projects/"+hosting, todoForm("Migrate DNS"))
	todo := c.todoPath("/projects/"+hosting, "Migrate DNS")
	moved := c.createEntry(withTodo(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "moved"), todo))
	cleared := c.createEntry(withTodo(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "cleared"), todo))
	list := c.get("/entries?day=2026-10-26")
	list.wantRow(t, "", "08:00–09:00", "1:00", "acme · hosting", "Migrate DNS", "moved", "yes")
	list.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "Migrate DNS", "cleared", "yes")

	c.postForm(moved, withTodo(entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "moved"), todo)).wantStatus(t, http.StatusSeeOther)
	c.postForm(cleared, entryForm(hosting, "2026-10-26T09:00", "2026-10-26T10:00", "cleared")).wantStatus(t, http.StatusSeeOther)

	list = c.get("/entries?day=2026-10-26")
	list.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "", "moved", "yes")
	list.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "", "cleared", "yes")
}

func TestADoneTodoIsNotPickedButStaysOnItsEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Launch"))
	c.createTodo(project, todoForm("Write copy"))
	launch, copy := c.todoPath(project, "Launch"), c.todoPath(project, "Write copy")
	kept := c.createEntry(withTodo(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "kept"), copy))
	other := c.createEntry(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "other"))
	done := url.Values{"status": {statusID(t, c.get(project), "Done")}}
	c.postForm(launch+"/status", done).wantStatus(t, http.StatusSeeOther)
	c.postForm(copy+"/status", done).wantStatus(t, http.StatusSeeOther)

	refused := c.postForm(other, withTodo(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "other"), launch))

	refused.wantStatus(t, http.StatusUnprocessableEntity)
	refused.wantBody(t, "This todo can&#39;t receive new time.")
	edit := c.get(kept)
	edit.wantBody(t, `<option value="`+todoID(copy)+`" selected>Write copy</option>`)
	edit.wantNoBody(t, ">Launch</option>")
	c.postForm(kept, withTodo(entryForm(strings.TrimPrefix(project, "/projects/"), "2026-10-26T08:00", "2026-10-26T08:30", "kept"), copy)).wantStatus(t, http.StatusSeeOther)
	c.get("/entries?day=2026-10-26").wantRow(t, "", "08:00–08:30", "0:30", "acme · website", "Write copy", "kept", "yes")
}

func TestTheTodoOfAClaimedEntryCannotChange(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createTodo("/projects/"+website, todoForm("Write copy"))
	todo := c.todoPath("/projects/"+website, "Write copy")
	entry := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	c.createDraft(pullForm(c, "project", "0"))

	res := c.postForm(entry, withTodo(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"), todo))

	res.wantStatus(t, http.StatusConflict)
	c.get("/entries?day=2026-09-02").wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "", "billed On a draft", "yes")
}

func TestSelectedEntriesGetATodoAndItsProjectTogether(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createTodo("/projects/"+hosting, todoForm("Migrate DNS"))
	todo := c.todoPath("/projects/"+hosting, "Migrate DNS")
	explicit := entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "explicit")
	explicit.Set("billable", "no")
	first := c.createEntry(explicit)
	second := c.createEntry(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "orphan"))
	c.createEntry(entryForm("", "2026-10-26T10:00", "2026-10-26T11:00", "left alone"))

	res := c.massEdit("todo", "?day=2026-10-26", url.Values{"todo": {todoID(todo)}}, first, second)

	res.wantStatus(t, http.StatusOK)
	res.wantRow(t, "", "08:00–09:00", "1:00", "acme · hosting", "Migrate DNS", "explicit", "no")
	res.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "Migrate DNS", "orphan", "yes")
	res.wantRow(t, "", "10:00–11:00", "1:00", "Unassigned", "", "left alone", "")
}

func TestEntriesFilterByTodoKeepingTheOtherFilters(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Write copy"))
	todo := c.todoPath(project, "Write copy")
	c.createEntry(withTodo(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "on the todo"), todo))
	c.createEntry(entryForm(strings.TrimPrefix(project, "/projects/"), "2026-10-26T09:00", "2026-10-26T10:00", "beside it"))

	res := c.get("/entries?day=2026-10-26&todo=" + todoID(todo))

	res.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "Write copy", "on the todo", "yes")
	res.wantNoBody(t, "beside it")
	res.wantBody(t, `<optgroup label="acme · website"><option value="`+todoID(todo)+`" selected>Write copy</option>`)
	res.wantBody(t, `<input type="hidden" name="todo" value="`+todoID(todo)+`">`)
	res.wantBody(t, `href="/entries?day=2026-10-26&amp;everyone=1&amp;todo=`+todoID(todo)+`">Team</a>`)
	res.wantBody(t, `formaction="/entries/todo?day=2026-10-26&amp;todo=`+todoID(todo)+`"`)
	c.get("/entries?day=2026-10-27&todo="+todoID(todo)).wantBody(t, "No time entries match these filters.")
	c.get("/entries?todo=999").wantStatus(t, http.StatusBadRequest)
	c.get("/entries?todo=x").wantStatus(t, http.StatusBadRequest)
}

func TestTodosOnEntriesRenderInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Texte schreiben"))
	entry := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "frei"))
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	list := c.get("/entries")
	edit := c.get(entry)

	wantInOrder(t, list.text(), "Alle Aufgaben", "Aufgabe setzen…", "Setzen")
	wantInOrder(t, edit.text(), "Aufgabe", "Keine Aufgabe", "Texte schreiben", "Eine Aufgabe setzt ihr Projekt.")
	c.get("/entries?todo=999").wantBody(t, "Ungültige Aufgabe.")
	c.massEdit("todo", "", url.Values{"todo": {"999"}}, entry).wantBody(t, "Auf diese Aufgabe kann keine neue Zeit gebucht werden.")
}

func TestSetTodoOffersTheOpenTodosByProjectAndKeepsTheListsQuery(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Launch"))
	c.createTodo(project, todoForm("Write copy"))
	launch, copy := c.todoPath(project, "Launch"), c.todoPath(project, "Write copy")
	c.postForm(launch+"/status", url.Values{"status": {statusID(t, c.get(project), "Done")}}).wantStatus(t, http.StatusSeeOther)
	c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "orphan"))

	page := c.get("/entries?day=2026-10-26")

	page.wantBody(t, `<optgroup label="acme · website"><option value="`+todoID(copy)+`">Write copy</option></optgroup>`)
	page.wantBody(t, `<option value="`+todoID(launch)+`">Launch</option>`)
	page.wantBody(t, `formaction="/entries/todo?day=2026-10-26"`)
}

func TestSettingATodoOnAClaimedEntryChangesNothing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createTodo("/projects/"+website, todoForm("Write copy"))
	todo := c.todoPath("/projects/"+website, "Write copy")
	claimed := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	c.createDraft(pullForm(c, "project", "0"))
	free := c.createEntry(entryForm("", "2026-09-03T08:00", "2026-09-03T09:00", "free"))

	res := c.massEdit("todo", "", url.Values{"todo": {todoID(todo)}}, free, claimed)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was changed. These time entries can't be changed:", "2026-09-02 08:00 “billed” · On a draft")
	wantPaths(t, res.selectable(), free)
	res.wantRow(t, "", "08:00–09:00", "1:00", "Unassigned", "", "free", "")
}

func TestSettingADoneOrUnknownTodoChangesNothing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websitePath()
	c.createTodo(project, todoForm("Launch"))
	launch := c.todoPath(project, "Launch")
	c.postForm(launch+"/status", url.Values{"status": {statusID(t, c.get(project), "Done")}}).wantStatus(t, http.StatusSeeOther)
	orphan := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "orphan"))

	for _, todo := range []string{todoID(launch), "999", "", "x"} {
		res := c.massEdit("todo", "", url.Values{"todo": {todo}}, orphan)

		res.wantStatus(t, http.StatusUnprocessableEntity)
		res.wantBody(t, "This todo can&#39;t receive new time.")
		res.wantRow(t, "", "08:00–09:00", "1:00", "Unassigned", "", "orphan", "")
	}
}

func TestAssigningSelectedEntriesToAnotherProjectClearsTheirTodo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createTodo("/projects/"+website, todoForm("Write copy"))
	todo := c.todoPath("/projects/"+website, "Write copy")
	stays := c.createEntry(withTodo(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "stays"), todo))
	moves := c.createEntry(withTodo(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "moves"), todo))

	c.massEdit("assign", "", url.Values{"project": {website}}, stays).wantStatus(t, http.StatusOK)
	res := c.massEdit("assign", "?day=2026-10-26", url.Values{"project": {hosting}}, moves)

	res.wantStatus(t, http.StatusOK)
	res.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "Write copy", "stays", "yes")
	res.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "", "moves", "yes")
}
