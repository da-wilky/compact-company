package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func TestArchivedProjectIsClosedToNewTime(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	wantRedirect(t, c.postForm("/projects/"+project+"/archive", nil), "/customers/1/projects")

	c.get("/customers/1/projects").wantNoBody(t, "website")
	c.get("/").wantNoBody(t, `<option value="`+project+`">`)
	c.get("/entries/new").wantNoBody(t, `<option value="`+project+`">`)
	c.hxPost("/timers", url.Values{"project": {project}}).wantStatus(t, http.StatusUnprocessableEntity)
	c.postForm("/entries", entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "late")).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestArchivingAProjectIsRefusedWhileATimerRunsOnIt(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	project := alice.websiteProject()
	timer := alice.startTimer(url.Values{"project": {project}})
	bob := ta.loginAs("bob")

	res := bob.postForm("/projects/"+project+"/archive", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "A timer is running on this project.")
	bob.get("/customers/1/projects").wantBody(t, "website")
	alice.hxPost(timer+"/stop", nil).wantStatus(t, http.StatusOK)
	wantRedirect(t, bob.postForm("/projects/"+project+"/archive", nil), "/customers/1/projects")
}

func TestArchivedProjectsAreListedOnRequestAndCanBeUnarchived(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.postForm("/projects/"+project+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	list := c.get("/customers/1/projects")
	list.wantNoBody(t, "website")
	list.wantBody(t, `href="/customers/1/projects?archived=1"`)
	all := c.get("/customers/1/projects?archived=1")
	all.wantBody(t, "website")
	all.wantBody(t, "(archived)")
	edit := c.get("/projects/" + project)
	edit.wantBody(t, `action="/projects/`+project+`/unarchive"`)
	edit.wantNoBody(t, `/archive"`)

	wantRedirect(t, c.postForm("/projects/"+project+"/unarchive", nil), "/customers/1/projects")

	c.get("/customers/1/projects").wantBody(t, "website")
	c.get("/").wantBody(t, `<option value="`+project+`">website</option>`)
	c.get("/projects/"+project).wantBody(t, `action="/projects/`+project+`/archive"`)
}

func TestArchivingACustomerArchivesItsProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	wantRedirect(t, c.postForm("/customers/1/archive", nil), "/customers")

	list := c.get("/customers")
	list.wantNoBody(t, "acme")
	list.wantBody(t, `href="/customers?archived=1"`)
	all := c.get("/customers?archived=1")
	all.wantBody(t, "acme")
	all.wantBody(t, "(archived)")
	c.get("/customers/1").wantBody(t, `action="/customers/1/unarchive"`)
	c.get("/customers/1/projects?archived=1").wantBody(t, "(archived)")
	c.get("/").wantNoBody(t, `<option value="`+project+`">`)
	c.hxPost("/timers", url.Values{"project": {project}}).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestProjectsOfAnArchivedCustomerStayClosedToNewTime(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.postForm("/customers/1/archive", nil).wantStatus(t, http.StatusSeeOther)

	c.postForm("/projects/"+project+"/unarchive", nil).wantStatus(t, http.StatusSeeOther)

	c.get("/entries/new").wantNoBody(t, `<option value="`+project+`">`)
	c.hxPost("/timers", url.Values{"project": {project}}).wantStatus(t, http.StatusUnprocessableEntity)
	c.postForm("/entries", entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "late")).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestUnarchivingACustomerKeepsItsNumber(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.websiteProject()
	c.postForm("/customers/1/archive", nil).wantStatus(t, http.StatusSeeOther)

	wantRedirect(t, c.postForm("/customers/1/unarchive", nil), "/customers")

	list := c.get("/customers")
	list.wantBody(t, "K-001")
	list.wantBody(t, "acme")
	c.get("/customers/1").wantBody(t, `action="/customers/1/archive"`)
}

func TestEntriesOfAnArchivedProjectStayVisibleAndEditable(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	entry := c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "kickoff"))
	other := c.createEntry(entryForm("", "2026-10-26T10:00", "2026-10-26T11:00", "orphan"))
	c.postForm("/customers/1/archive", nil).wantStatus(t, http.StatusSeeOther)

	c.get("/entries?day=2026-10-26").wantBody(t, "acme · website")
	c.get(entry).wantBody(t, `<option value="`+project+`" selected>website</option>`)
	c.get(other).wantNoBody(t, `<option value="`+project+`"`)
	res := c.postForm(entry, entryForm(project, "2026-10-26T08:00", "2026-10-26T09:30", "kickoff, longer"))

	wantRedirect(t, res, "/entries?day=2026-10-26")
	c.get("/entries?day=2026-10-26").wantBody(t, "08:00–09:30")
	c.postForm(other, entryForm(project, "2026-10-26T10:00", "2026-10-26T11:00", "orphan")).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestProjectCanBeDeletedOnlyWithoutTimeEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.get("/projects/"+project).wantBody(t, `action="/projects/`+project+`/delete"`)
	entry := c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "kickoff"))

	res := c.postForm("/projects/"+project+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project has time entries.")
	c.postForm(entry+"/delete", nil).wantStatus(t, http.StatusSeeOther)
	wantRedirect(t, c.postForm("/projects/"+project+"/delete", nil), "/customers/1/projects")
	c.get("/customers/1/projects?archived=1").wantNoBody(t, "website")
	c.get("/projects/"+project).wantStatus(t, http.StatusNotFound)
}

func TestProjectWithARunningTimerCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.startTimer(url.Values{"project": {project}})

	c.postForm("/projects/"+project+"/delete", nil).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestCustomerCanBeDeletedOnlyWithoutProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.get("/customers/1").wantBody(t, `action="/customers/1/delete"`)

	res := c.postForm("/customers/1/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This customer has projects.")
	c.postForm("/projects/"+project+"/delete", nil).wantStatus(t, http.StatusSeeOther)
	wantRedirect(t, c.postForm("/customers/1/delete", nil), "/customers")
	c.get("/customers?archived=1").wantNoBody(t, "acme")
	c.get("/customers/1").wantStatus(t, http.StatusNotFound)
}

func TestDeletedCustomersNumberIsNeverReused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))
	c.postForm("/customers/2/delete", nil).wantStatus(t, http.StatusSeeOther)

	c.createCustomer(customerForm("initech", "95"))

	list := c.get("/customers")
	list.wantNoBody(t, "K-002")
	list.wantBody(t, "K-003")
	list.wantBody(t, "initech")
}

func TestArchivingACustomerIsRefusedWhileATimerRunsOnOneOfItsProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	project := alice.websiteProject()
	alice.startTimer(url.Values{"project": {project}})
	bob := ta.loginAs("bob")

	res := bob.postForm("/customers/1/archive", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "A timer is running on a project of this customer.")
	bob.get("/customers").wantBody(t, "acme")
	bob.get("/customers/1/projects").wantBody(t, "website")
}
