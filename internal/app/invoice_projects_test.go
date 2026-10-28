package app_test

import (
	"net/http"
	"strings"
	"testing"
)

func customerID(c *client, name string) string {
	return strings.TrimPrefix(c.customerPath(name), "/customers/")
}

func projectChoice(id string) string {
	return `name="project" value="` + id + `"`
}

func TestNewDraftFormBillsAllProjectsByDefaultAndListsTheCustomersProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.postForm("/projects/"+hosting+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("intranet", ""))
	intranet := strings.TrimPrefix(c.projectPath(c.customerPath("globex"), "intranet"), "/projects/")

	res := c.get("/invoices/new?customer=" + customerID(c, "acme"))

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `name="all_projects" value="1" checked`)
	wantInOrder(t, res.body, "All projects", projectChoice(hosting), "archived", projectChoice(website))
	res.wantNoBody(t, projectChoice(intranet))

	res = c.get("/invoices/new?customer=" + customerID(c, "globex"))
	res.wantBody(t, projectChoice(intranet))
	res.wantNoBody(t, projectChoice(website))
}

func TestDraftWithChosenProjectsPullsAndRefreshesOnlyTheirEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createProject(c.customerPath("acme"), projectForm("support", ""))
	support := strings.TrimPrefix(c.projectPath(c.customerPath("acme"), "support"), "/projects/")
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T09:00", "server"))
	form := pullForm(c, "entry", "0")
	form.Del("all_projects")
	form["project"] = []string{website, support}

	draft := c.createDraft(form)

	res := c.get(draft)
	res.wantBody(t, ">design<")
	res.wantNoBody(t, ">server<")
	wantInOrder(t, res.text(), "Projects", "support", "website")
	res.wantNoBody(t, "All projects")

	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T09:00", "review"))
	c.createEntry(entryForm(hosting, "2026-09-05T08:00", "2026-09-05T09:00", "backup"))
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res = c.get(draft)
	wantInOrder(t, res.body, ">design<", ">review<")
	res.wantNoBody(t, ">server<")
	res.wantNoBody(t, ">backup<")
	wantInOrder(t, res.text(), "Projects", "support", "website")
}

func TestDraftRefusesAnotherCustomersProjectAndAnEmptyChoice(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("intranet", ""))
	intranet := strings.TrimPrefix(c.projectPath(c.customerPath("globex"), "intranet"), "/projects/")

	for name, projects := range map[string][]string{"foreign": {website, intranet}, "none": nil, "garbage": {"x"}} {
		t.Run(name, func(t *testing.T) {
			form := pullForm(c, "project", "0")
			form.Del("all_projects")
			form["project"] = projects

			res := c.postForm("/invoices", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, "Tick at least one of the customer")
		})
	}
	c.get("/invoices").wantBody(t, "No invoices.")
}

func TestChosenProjectWithoutEntriesStaysChosenUntilDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	form := pullForm(c, "entry", "0")
	form.Del("all_projects")
	form["project"] = []string{website, hosting}

	draft := c.createDraft(form)
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res := c.get(draft)
	res.wantBody(t, ">design<")
	wantInOrder(t, res.text(), "Projects", "hosting, website")

	wantRedirect(t, c.postForm("/projects/"+hosting+"/delete", nil), c.customerPath("acme")+"/projects")

	wantInOrder(t, c.get(draft).text(), "Projects", "website")
	c.get(draft).wantNoBody(t, "hosting")
}

func TestDraftForAllProjectsRefreshesFromProjectsAddedLater(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	form := pullForm(c, "entry", "0")
	// The hidden project checkboxes still submit; "All projects" overrides them.
	form["project"] = []string{website}

	draft := c.createDraft(form)
	c.get(draft).wantBody(t, "All projects")
	c.createProject(c.customerPath("acme"), projectForm("support", ""))
	support := strings.TrimPrefix(c.projectPath(c.customerPath("acme"), "support"), "/projects/")
	c.createEntry(entryForm(support, "2026-09-03T08:00", "2026-09-03T09:00", "hotline"))
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	wantInOrder(t, c.get(draft).body, ">design<", ">hotline<")
}
