package app_test

import (
	"net/http"
	"testing"
)

func TestInvoiceCustomerFilterMarksArchivedAndKeepsTheStatus(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))
	acme, globex := customerID(c, "acme"), customerID(c, "globex")
	wantRedirect(t, c.postForm(c.customerPath("globex")+"/archive", nil), "/customers")

	res := c.get("/invoices?status=draft&customer=" + acme)

	wantInOrder(t, res.body, `<option value="">All customers</option>`, `<option value="`+acme+`" selected>acme</option>`, `<option value="`+globex+`">globex (archived)</option>`)
	res.wantBody(t, `<input type="hidden" name="status" value="draft">`)
	res.wantNoBody(t, `<input type="hidden" name="customer"`)
	res.wantBody(t, `href="/invoices?customer=`+acme+`&amp;status=issued">Issued</a>`)
	res.wantBody(t, `href="/invoices?customer=`+acme+`">All</a>`)
	res.wantNoBody(t, ">Filter</button>")
}

func TestEntryProjectFilterListsEveryProjectAndKeepsTheOtherFilters(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websiteProject()
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("legacy", ""))
	legacy := c.projectPath(cust, "legacy")
	wantRedirect(t, c.postForm(legacy+"/archive", nil), cust+"/projects")

	res := c.get("/entries?day=2026-10-26&everyone=1&per_page=25&project=" + website)

	wantInOrder(t, res.body, `<option value="">All projects</option>`, `<option value="none">Unassigned</option>`,
		`<optgroup label="acme">`, `">legacy (archived)</option>`, `<option value="`+website+`" selected>website</option>`)
	for _, hidden := range []string{`name="day" value="2026-10-26"`, `name="everyone" value="1"`, `name="per_page" value="25"`} {
		res.wantBody(t, `<input type="hidden" `+hidden+`>`)
	}
	c.get("/entries?project=none").wantBody(t, `<option value="none" selected>Unassigned</option>`)
	c.get("/entries?project=999").wantStatus(t, http.StatusBadRequest)
}

func TestEntryProjectFilterDropsTheCursor(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/entries?before=1761469200-1&per_page=25")

	res.wantBody(t, `<input type="hidden" name="per_page" value="25">`)
	res.wantNoBody(t, `name="before"`)
}

func TestWeekTogglesTeamKeepingTheWeek(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/week?week=2026-10-19").wantBody(t, `href="/week?everyone=1&amp;week=2026-10-19">Team</a>`)
	c.get("/week?week=2026-10-19&everyone=1").wantBody(t, `href="/week?week=2026-10-19">Mine</a>`)
}
