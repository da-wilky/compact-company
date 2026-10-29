package app_test

import "testing"

func TestEmptyListsSayNothingExistsYet(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/customers").wantBody(t, "No customers yet.")
	c.get("/entries").wantBody(t, "You have no time entries yet.")
	c.get("/entries?everyone=1").wantBody(t, "No time entries yet.")
	c.get("/invoices").wantBody(t, "No invoices yet.")
	c.get("/todos").wantBody(t, "No todos yet.")
	c.createCustomer(customerForm("acme", "85"))
	c.get(c.customerPath("acme")+"/projects").wantBody(t, "No projects yet.")
}

func TestEmptyListsNameTheFilterThatHidesEverything(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	cust := c.customerPath("acme")
	c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "kickoff"))
	c.createTodo("/projects/"+project, todoForm("Write copy"))
	wantRedirect(t, c.postForm(cust+"/archive", nil), "/customers")

	c.get("/customers").wantBody(t, "No active customers.")
	c.get(cust+"/projects").wantBody(t, "No active projects.")
	c.get("/todos").wantBody(t, "No todos in active projects.")
	c.get("/entries?day=2026-10-27").wantBody(t, "You have no time entries on this day.")
	c.get("/entries?day=2026-10-27&everyone=1").wantBody(t, "No time entries on this day.")
	c.get("/entries?day=2026-10-26&project=none").wantBody(t, "No time entries match these filters.")
	c.get("/entries?project=none").wantBody(t, "No time entries match these filters.")
	c.get("/invoices?status=paid").wantBody(t, "No invoices match these filters.")
}
