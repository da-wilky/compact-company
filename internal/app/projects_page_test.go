package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
)

func TestProjectsPageListsEveryCustomersProjectsAndFiltersByCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "100"))
	acme, globex := c.customerPath("acme"), c.customerPath("globex")
	c.createProject(acme, projectForm("website", ""))
	c.createProject(globex, projectForm("audit", "120"))

	all := c.get("/projects")
	all.wantStatus(t, http.StatusOK)
	all.wantRow(t, "", "acme", "website", "yes", "85.00 EUR (customer default)")
	all.wantRow(t, "", "globex", "audit", "yes", "120.00 EUR")

	id := strings.TrimPrefix(globex, "/customers/")
	filtered := c.get("/projects?customer=" + id)
	filtered.wantRow(t, "", "globex", "audit", "yes", "120.00 EUR")
	filtered.wantNoBody(t, ">website</a>")
	filtered.wantBody(t, `href="/projects/new?customer=`+id+`"`)
	filtered.wantBody(t, `href="/projects?archived=1&amp;customer=`+id+`"`)

	c.get("/projects?customer=x").wantStatus(t, http.StatusBadRequest)
}

func TestProjectsPageArchivesSelectedProjectsAcrossCustomers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "100"))
	acme, globex := c.customerPath("acme"), c.customerPath("globex")
	c.createProject(acme, projectForm("website", ""))
	c.createProject(globex, projectForm("audit", ""))
	ids := url.Values{"id": {
		strings.TrimPrefix(c.projectPath(acme, "website"), "/projects/"),
		strings.TrimPrefix(c.projectPath(globex, "audit"), "/projects/"),
	}}

	c.postForm("/projects/archive", ids).wantStatus(t, http.StatusOK)

	list := c.get("/projects")
	list.wantBody(t, "No active projects.")
	all := c.get("/projects?archived=1")
	all.wantBody(t, "website")
	all.wantBody(t, "audit")
}

func TestNewProjectFromTheProjectsPagePicksAnActiveCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("oldco", "85"))
	acme, oldco := c.customerPath("acme"), c.customerPath("oldco")
	c.postForm(oldco+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	acmeID, oldcoID := strings.TrimPrefix(acme, "/customers/"), strings.TrimPrefix(oldco, "/customers/")

	form := c.get("/projects/new?customer=" + acmeID)
	form.wantStatus(t, http.StatusOK)
	form.wantBody(t, `<option value="`+acmeID+`" selected>acme</option>`)
	form.wantNoBody(t, "oldco")

	refused := projectForm("ghost", "")
	refused.Set("customer", oldcoID)
	res := c.postForm("/projects", refused)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This customer is archived.")
	res.wantBody(t, `value="ghost"`)

	created := projectForm("website", "90")
	created.Set("customer", acmeID)
	wantRedirect(t, c.postForm("/projects", created), "/projects")
	c.get("/projects").wantRow(t, "", "acme", "website", "yes", "90.00 EUR")
}

func TestAnArchivedCustomerGetsNoNewProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("oldco", "85"))
	oldco := c.customerPath("oldco")
	c.postForm(oldco+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	res := c.postForm(oldco+"/projects", projectForm("ghost", ""))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This customer is archived.")
	c.get(oldco+"/projects?archived=1").wantNoBody(t, ">ghost</a>")

	missing := projectForm("ghost", "")
	missing.Set("customer", "999")
	res = c.postForm("/projects", missing)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Pick a customer.")
}
