package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"testing"
)

func projectForm(name, rate string) url.Values {
	return url.Values{"name": {name}, "hourly_rate": {rate}, "billable": {"on"}}
}

func (c *client) createProject(customerPath string, form url.Values) {
	c.t.Helper()
	res := c.postForm(customerPath+"/projects", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != customerPath+"/projects" {
		c.t.Fatalf("Location = %q, want %s/projects", loc, customerPath)
	}
}

func TestProjectWithoutRateUsesTheCustomersRate(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")

	c.createProject(cust, projectForm("website", ""))

	res := c.get(cust + "/projects")
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "website")
	res.wantBody(t, "85.00 EUR")
}

func TestEffectiveRateFollowsTheCustomersRateUnlessOverridden(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	c.createProject(cust, projectForm("audit", "120"))

	c.postForm(cust, customerForm("acme", "95")).wantStatus(t, http.StatusSeeOther)

	res := c.get(cust + "/projects")
	res.wantRow(t, "", "website", "yes", "95.00 EUR (customer default)")
	res.wantRow(t, "", "audit", "yes", "120.00 EUR")
	res.wantNoBody(t, "85.00")
}

func TestProjectRateOverrideIsInTheCustomersCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	form := customerForm("yenco", "8000")
	form.Set("currency", "JPY")
	c.createCustomer(form)
	cust := c.customerPath("yenco")

	c.createProject(cust, projectForm("audit", "12000"))
	res := c.postForm(cust+"/projects", projectForm("cents", "100.5"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Hourly rate must be")
	res.wantBody(t, `value="cents"`)
	list := c.get(cust + "/projects")
	list.wantBody(t, "12000 JPY")
	list.wantNoBody(t, "cents")
}

func TestNewProjectIsBillableByDefault(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")

	res := c.get(cust + "/projects/new")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `name="billable" checked`)
	res.wantBody(t, "85.00 EUR")
}

func TestProjectCanBeNonBillable(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	internal := projectForm("internal", "")
	internal.Del("billable")

	c.createProject(cust, internal)
	c.createProject(cust, projectForm("website", ""))

	res := c.get(cust + "/projects")
	res.wantRow(t, "", "internal", "no", "85.00 EUR (customer default)")
	res.wantRow(t, "", "website", "yes", "85.00 EUR (customer default)")
}

var projectLink = regexp.MustCompile(`<a href="(/projects/\d+)">([^<]*)</a>`)

func (c *client) projectPath(customerPath, name string) string {
	c.t.Helper()
	for _, m := range projectLink.FindAllStringSubmatch(c.get(customerPath+"/projects").body, -1) {
		if m[2] == name {
			return m[1]
		}
	}
	c.t.Fatalf("no project %q under %s", name, customerPath)
	return ""
}

func TestAnyUserCanEditAnyProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	bob := ta.loginAs("bob")
	alice.createCustomer(customerForm("acme", "85"))
	cust := alice.customerPath("acme")
	alice.createProject(cust, projectForm("website", "100"))
	path := bob.projectPath(cust, "website")

	edit := bob.get(path)
	edit.wantStatus(t, http.StatusOK)
	edit.wantBody(t, "Project website")
	edit.wantBody(t, `value="100.00"`)
	edit.wantBody(t, `name="billable" checked`)
	form := projectForm("relaunch", "")
	form.Del("billable")
	res := bob.postForm(path, form)

	res.wantStatus(t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != cust+"/projects" {
		t.Fatalf("Location = %q, want %s/projects", loc, cust)
	}
	list := alice.get(cust + "/projects")
	list.wantRow(t, "", "relaunch", "no", "85.00 EUR (customer default)")
	list.wantBody(t, "85.00 EUR")
	list.wantNoBody(t, "website")
}

func TestInvalidProjectEditKeepsTheProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", "100"))
	path := c.projectPath(cust, "website")

	res := c.postForm(path, projectForm("", "100"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Name is required.")
	c.get(cust+"/projects").wantBody(t, "website")
}

func TestCustomerListLinksToItsProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	c.get("/customers").wantBody(t, `href="`+c.customerPath("acme")+`/projects"`)
}

func TestUnknownProjectOrCustomerIsNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/customers/99/projects").wantStatus(t, http.StatusNotFound)
	c.get("/customers/99/projects/new").wantStatus(t, http.StatusNotFound)
	c.postForm("/customers/99/projects", projectForm("website", "")).wantStatus(t, http.StatusNotFound)
	c.get("/projects/99").wantStatus(t, http.StatusNotFound)
	c.get("/projects/abc").wantStatus(t, http.StatusNotFound)
	c.postForm("/projects/99", projectForm("website", "")).wantStatus(t, http.StatusNotFound)
}

func TestProjectPagesInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
`)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")

	c.get(cust+"/projects").wantBody(t, "Noch keine Projekte.")
	c.postForm(cust+"/projects", projectForm("website", "x")).wantBody(t, "Stundensatz muss")
}

func TestCurrencyChangeKeepsTheDecimalRateOverrides(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("audit", "120.50"))
	c.createProject(cust, projectForm("website", ""))
	form := customerForm("acme", "90")
	form.Set("currency", "JPY")

	c.postForm(cust, form).wantStatus(t, http.StatusSeeOther)

	res := c.get(cust + "/projects")
	res.wantRow(t, "", "audit", "yes", "121 JPY")
	res.wantRow(t, "", "website", "yes", "90 JPY (customer default)")

	form.Set("currency", "EUR")
	c.postForm(cust, form).wantStatus(t, http.StatusSeeOther)
	c.get(cust+"/projects").wantRow(t, "", "audit", "yes", "121.00 EUR")
}
