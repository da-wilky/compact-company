package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
)

// selectIDs is the form a selection posts: the ids of the paths' last segments.
func selectIDs(paths ...string) url.Values {
	form := url.Values{}
	for _, p := range paths {
		form.Add("id", p[strings.LastIndex(p, "/")+1:])
	}
	return form
}

func TestSelectedCustomersAreArchivedTogether(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for _, name := range []string{"acme", "globex", "initech"} {
		c.createCustomer(customerForm(name, "85"))
	}
	acme, globex := c.customerPath("acme"), c.customerPath("globex")
	c.get("/customers").wantBody(t, `formaction="/customers/archive"`)

	wantRedirect(t, c.postForm("/customers/archive", selectIDs(acme, globex)), "/customers")

	list := c.get("/customers")
	list.wantNoBody(t, "acme")
	list.wantNoBody(t, "globex")
	list.wantBody(t, "initech")
}

func TestSelectedCustomersAreUnarchivedKeepingTheArchivedFilter(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "85"))
	acme, globex := c.customerPath("acme"), c.customerPath("globex")
	wantRedirect(t, c.postForm("/customers/archive", selectIDs(acme, globex)), "/customers")
	all := c.get("/customers?archived=1")
	all.wantBody(t, `formaction="/customers/unarchive?archived=1"`)
	all.wantBody(t, `value="1" form="customer-selection" data-state="archived"`)

	wantRedirect(t, c.postForm("/customers/unarchive?archived=1", selectIDs(acme)), "/customers?archived=1")

	list := c.get("/customers")
	list.wantBody(t, "acme")
	list.wantNoBody(t, "globex")
}

func TestABlockedCustomerStopsTheWholeMassDeleteAndIsNamed(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.websiteProject()
	c.createCustomer(customerForm("globex", "85"))
	c.createCustomer(customerForm("initech", "85"))
	acme, globex, initech := c.customerPath("acme"), c.customerPath("globex"), c.customerPath("initech")

	res := c.postForm("/customers/delete", selectIDs(acme, globex))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was deleted. These customers can't be deleted:", "K-001 acme · This customer has projects.")
	res.wantNoBody(t, "globex · ")
	res.wantBody(t, `aria-label="Select customer K-002 globex" checked`)
	res.wantNoBody(t, `aria-label="Select customer K-003 initech" checked`)
	c.get(globex).wantStatus(t, http.StatusOK)

	wantRedirect(t, c.postForm("/customers/delete", selectIDs(globex, initech)), "/customers")
	c.get(globex).wantStatus(t, http.StatusNotFound)
	c.get(initech).wantStatus(t, http.StatusNotFound)
}

func TestARunningTimerStopsMassArchivingItsCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.createCustomer(customerForm("globex", "85"))
	c.startTimer(url.Values{"project": {project}})

	res := c.postForm("/customers/archive", selectIDs(c.customerPath("acme"), c.customerPath("globex")))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was archived.", "acme · A timer is running on a project of this customer.")
	c.get("/customers").wantBody(t, "globex")
}

func TestUnknownSelectedCustomerIsNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	c.postForm("/customers/delete", selectIDs(c.customerPath("acme"), "/customers/99")).wantStatus(t, http.StatusNotFound)

	c.get("/customers").wantBody(t, "acme")
}

func TestSelectedProjectsAreArchivedAndUnarchivedTogether(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	cust := c.customerPath("acme")
	c.get(cust+"/projects").wantBody(t, `formaction="`+cust+`/projects/archive"`)

	wantRedirect(t, c.postForm(cust+"/projects/archive", selectIDs(website, hosting)), cust+"/projects")

	c.get(cust+"/projects").wantNoBody(t, "website")
	c.get(cust+"/projects").wantNoBody(t, "hosting")
	wantRedirect(t, c.postForm(cust+"/projects/unarchive?archived=1", selectIDs(website)), cust+"/projects?archived=1")
	c.get(cust+"/projects").wantBody(t, "website")
	c.get(cust+"/projects").wantNoBody(t, "hosting")
}

func TestBlockedProjectsStopTheWholeMassDeleteAndAreNamed(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("audit", ""))
	audit := c.projectPath(cust, "audit")
	c.createEntry(entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "design"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))

	res := c.postForm(cust+"/projects/delete", selectIDs(website, hosting, audit))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was deleted. These projects can't be deleted:",
		"website · This project has time entries.", "hosting · This project is on an invoice.")
	res.wantNoBody(t, "audit · ")
	c.get(audit).wantStatus(t, http.StatusOK)

	wantRedirect(t, c.postForm(cust+"/projects/delete", selectIDs(audit)), cust+"/projects")
	c.get(audit).wantStatus(t, http.StatusNotFound)
}

func TestARunningTimerStopsMassArchivingItsProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	cust := c.customerPath("acme")
	c.startTimer(url.Values{"project": {hosting}})

	res := c.postForm(cust+"/projects/archive", selectIDs(website, hosting))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was archived.", "hosting · A timer is running on this project.")
	c.get(cust+"/projects").wantBody(t, "website")
}

func TestAnotherCustomersProjectCantBeSelected(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websiteProject()
	c.createCustomer(customerForm("globex", "85"))

	c.postForm(c.customerPath("globex")+"/projects/delete", selectIDs(website)).wantStatus(t, http.StatusNotFound)

	c.get("/projects/"+website).wantStatus(t, http.StatusOK)
}
