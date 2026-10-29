package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

func TestTodosPageShowsEveryProjectsTodosNamingTheProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websitePath()
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("hosting", ""))
	hosting := c.projectPath(cust, "hosting")
	c.createTodo(website, todoForm("Write copy"))
	c.createTodo(hosting, todoForm("Renew certificate"))

	res := c.get("/todos")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `href="/todos">`)
	open := res.column(t, "Open")
	wantCards(t, open, "Write copy", "Renew certificate")
	wantInOrder(t, open.text(), "Write copy", "website", "Renew certificate", "hosting")
	res.wantBody(t, `<input type="hidden" name="return" value="/todos">`)
}

func TestTodosPageHidesTodosOfArchivedProjectsAndCustomers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websitePath()
	acme := c.customerPath("acme")
	c.createProject(acme, projectForm("legacy", ""))
	legacy := c.projectPath(acme, "legacy")
	c.createCustomer(customerForm("globex", "90"))
	globex := c.customerPath("globex")
	c.createProject(globex, projectForm("shop", ""))
	shop := c.projectPath(globex, "shop")
	c.createTodo(website, todoForm("Write copy"))
	c.createTodo(legacy, todoForm("Migrate data"))
	c.createTodo(shop, todoForm("Fix cart"))

	wantRedirect(t, c.postForm(legacy+"/archive", nil), acme+"/projects")
	wantRedirect(t, c.postForm(globex+"/archive", nil), "/customers")

	res := c.get("/todos")
	wantCards(t, res, "Write copy")
	res.wantNoBody(t, `name="archived"`)
}

func TestTodosPageFiltersByCustomerProjectAssigneeAndStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website := alice.websitePath()
	acme := alice.customerPath("acme")
	alice.createCustomer(customerForm("globex", "90"))
	globex := alice.customerPath("globex")
	alice.createProject(globex, projectForm("shop", ""))
	shop := alice.projectPath(globex, "shop")
	bobID := regexp.MustCompile(`<option value="(\d+)">bob</option>`).FindStringSubmatch(alice.get("/todos").body)[1]
	assigned := func(project, title, assignee string) {
		form := todoForm(title)
		form.Set("assignee", assignee)
		alice.createTodo(project, form)
	}
	assigned(website, "Write copy", bobID)
	assigned(website, "Fix footer", "")
	assigned(shop, "Fix cart", bobID)
	alice.postForm(alice.todoPath(website, "Fix footer")+"/status", url.Values{"status": {statusID(t, alice.get(website), "Waiting")}}).wantStatus(t, http.StatusSeeOther)
	waiting := statusID(t, alice.get(website), "Waiting")
	acmeID, websiteID, shopID := strings.TrimPrefix(acme, "/customers/"), strings.TrimPrefix(website, "/projects/"), strings.TrimPrefix(shop, "/projects/")

	wantCards(t, alice.get("/todos?customer="+acmeID), "Write copy", "Fix footer")
	wantCards(t, alice.get("/todos?project="+shopID), "Fix cart")
	wantCards(t, alice.get("/todos?status="+waiting), "Fix footer")
	wantCards(t, alice.get("/todos?assignee="+bobID+"&customer="+acmeID), "Write copy")
	wantCards(t, bob.get("/todos?assignee=me"), "Write copy", "Fix cart")
	wantCards(t, alice.get("/todos?assignee=me"))

	res := alice.get("/todos?assignee=me&project=" + websiteID)
	wantInOrder(t, res.text(), "No todos match these filters.")
	res.wantBody(t, `<option value="me" selected>Me</option>`)
	res.wantBody(t, `<option value="`+websiteID+`" selected>website</option>`)
	res.wantBody(t, `<input type="hidden" name="assignee" value="me">`)
	res.wantBody(t, `<input type="hidden" name="project" value="`+websiteID+`">`)
	bob.get("/todos?assignee=me&project="+websiteID).wantBody(t, `<input type="hidden" name="return" value="/todos?assignee=me&amp;project=`+websiteID+`">`)
	wantInOrder(t, res.body, `<option value="">All customers</option>`, `<option value="`+acmeID+`">acme</option>`,
		`<option value="">All projects</option>`, `<optgroup label="acme">`, `<optgroup label="globex">`,
		`<option value="">Everyone</option>`, `<option value="me" selected>Me</option>`, `>alice</option>`, `>bob</option>`,
		`<option value="">All statuses</option>`, `>Open</option>`, `>Done</option>`)

	for _, bad := range []string{"customer=x", "project=x", "assignee=x", "status=x"} {
		alice.get("/todos?"+bad).wantStatus(t, http.StatusBadRequest)
	}
}

func TestTodosPageStatusChangeReturnsToTheFilteredBoard(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websitePath()
	c.createTodo(website, todoForm("Write copy"))
	ret := "/todos?project=" + strings.TrimPrefix(website, "/projects/")

	res := c.hxPost(c.todoPath(website, "Write copy")+"/status", url.Values{"status": {statusID(t, c.get(website), "Done")}, "return": {ret}})

	wantRedirect(t, res, ret)
	wantCards(t, c.get(ret).column(t, "Done"), "Write copy")
}

func TestTodosPageShowsTheEmptyHint(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	wantInOrder(t, c.get("/todos").text(), "No todos yet.")
}

func TestTodosPageRendersInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	res := c.get("/todos?assignee=me")

	res.wantBody(t, `href="/todos">`)
	wantInOrder(t, res.text(), "Aufgaben", "Alle Kunden", "Alle Projekte", "Ich", "Alle Status", "Keine Aufgaben passen zu diesen Filtern.")
}
