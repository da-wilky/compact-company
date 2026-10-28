package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

func TestOverviewSumsEveryonesEndedBillableTimePerCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, hosting := alice.acmeProjects()
	alice.createEntry(entryForm(website, "2026-10-05T08:00", "2026-10-05T10:00", "alice's"))
	bob.createEntry(entryForm(hosting, "2026-10-06T08:00", "2026-10-06T08:30", "bob's"))
	internal := entryForm(website, "2026-10-07T08:00", "2026-10-07T12:00", "internal")
	internal.Set("billable", "no")
	alice.createEntry(internal)
	alice.createEntry(entryForm("", "2026-10-08T08:00", "2026-10-08T12:00", "unassigned"))
	bob.startTimer(url.Values{"project": {website}})
	ta.clock.Advance(3 * time.Hour)

	res := alice.get("/")

	res.wantRow(t, "acme", "2:30 h", "", "2026-10-05", "Create draft")
	res.wantNoBody(t, "Nothing to bill, nothing open.")
}

func TestOverviewSplitsUnbilledTimeIntoNotOnAnyInvoiceAndEachDraft(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T10:00", "early september"))
	c.createEntry(entryForm(website, "2026-09-20T08:00", "2026-09-20T08:30", "late september"))
	c.createEntry(entryForm(website, "2026-10-05T08:00", "2026-10-05T09:00", "october"))
	early := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-15"))
	late := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-16", "2026-09-30"))

	res := c.get("/")

	res.wantRow(t, "acme", "1:00 h", "2:00 h , 0:30 h", "2026-09-02", "Create draft")
	res.wantBody(t, `<a href="`+early+`">2:00 h</a>`)
	res.wantBody(t, `<a href="`+late+`">0:30 h</a>`)
}

func TestOverviewLeavesOutTimeOnIssuedInvoicesAndDatesTheOldestEntryInTheCustomersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T10:00", "issued"))
	// 22:30 UTC on 30 September is 1 October in acme's Europe/Berlin.
	c.createEntry(entryForm(website, "2026-09-30T22:30", "2026-09-30T23:00", "berlin october"))
	c.issue(c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))).wantStatus(t, http.StatusSeeOther)

	res := c.get("/")

	res.wantRow(t, "acme", "0:30 h", "", "2026-10-01", "Create draft")
}

func TestOverviewOffersCreateDraftOnlyForTimeNotOnAnyInvoice(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T10:00", "on draft"))
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))

	res := c.get("/")

	res.wantRow(t, "acme", "", "2:00 h", "2026-09-02", "")
	res.wantNoBody(t, "/invoices/new?customer=")
	c.postForm(draft+"/delete", nil).wantStatus(t, http.StatusSeeOther)
	c.get("/").wantBody(t, `<a href="/invoices/new?customer=`)
}

func TestOverviewListsDraftsWithCustomerAndRange(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.get("/").wantNoBody(t, "Drafts")
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	issued := c.acmeDraft()
	c.addLine(issued, lineForm("Workshop", "1", "85"))
	c.issue(issued).wantStatus(t, http.StatusSeeOther)
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))

	res := c.get("/")

	drafts := res.in(t, "Drafts")
	drafts.wantRow(t, "acme", "2026-10-01 – 2026-10-31")
	drafts.wantBody(t, `<a href="`+draft+`">`)
	drafts.wantNoBody(t, "2026-09-01 – 2026-09-30")
	res.wantNoBody(t, "Nothing to bill, nothing open.")
}

func TestNewDraftFormPreselectsTheCustomerNamedInTheQuery(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))
	globex := strings.TrimPrefix(c.customerPath("globex"), "/customers/")

	res := c.get("/invoices/new?customer=" + globex)

	res.wantBody(t, `<option value="`+globex+`" selected>globex</option>`)
	res.wantBody(t, `name="from" value="2026-09-01"`)
	res.wantBody(t, `name="to" value="2026-09-30"`)
	for _, other := range []string{"999", "x", ""} {
		c.get("/invoices/new?customer="+other).wantNoBody(t, " selected>globex<")
	}
}

func TestOverviewListsCustomersWithUnbilledTimeArchivedOnesIncludedMostHoursFirst(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	for _, name := range []string{"globex", "initech"} {
		c.createCustomer(customerForm(name, "90"))
		c.createProject(c.customerPath(name), projectForm("intranet", ""))
	}
	globex := strings.TrimPrefix(c.projectPath(c.customerPath("globex"), "intranet"), "/projects/")
	c.createEntry(entryForm(website, "2026-10-05T08:00", "2026-10-05T09:00", "acme"))
	c.createEntry(entryForm(globex, "2026-10-06T08:00", "2026-10-06T11:00", "globex"))
	c.postForm(c.customerPath("globex")+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	res := c.get("/")

	wantInOrder(t, res.text(), "globex 3:00 h", "acme 1:00 h")
	res.in(t, "Unbilled time").wantNoBody(t, "initech")
}
