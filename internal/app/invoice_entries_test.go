package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

// acmeProjects creates customer acme (rate 85, Europe/Berlin) with projects website (customer rate)
// and hosting (rate 100) and returns their ids.
func (c *client) acmeProjects() (website, hosting string) {
	c.t.Helper()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	c.createProject(cust, projectForm("hosting", "100"))
	return strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"),
		strings.TrimPrefix(c.projectPath(cust, "hosting"), "/projects/")
}

func pullForm(c *client, grouping, rounding string) url.Values {
	form := draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30")
	form.Set("grouping", grouping)
	form.Set("rounding_minutes", rounding)
	return form
}

func TestDraftPullsTheCustomersUnbilledBillableEndedEntriesPerProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, hosting := alice.acmeProjects()
	alice.createCustomer(customerForm("globex", "90"))
	globex := alice.customerPath("globex")
	alice.createProject(globex, projectForm("intranet", ""))
	intranet := strings.TrimPrefix(alice.projectPath(globex, "intranet"), "/projects/")
	// Users enter UTC; acme is billed in Europe/Berlin, two hours ahead in September.
	alice.createEntry(entryForm(website, "2026-08-31T22:30", "2026-08-31T23:30", "first Berlin hour"))
	alice.createEntry(entryForm(website, "2026-08-31T21:30", "2026-08-31T21:45", "still August in Berlin"))
	alice.createEntry(entryForm(website, "2026-09-30T21:00", "2026-09-30T21:30", "last Berlin half hour"))
	alice.createEntry(entryForm(website, "2026-09-30T22:00", "2026-09-30T22:45", "October in Berlin"))
	bob.createEntry(entryForm(website, "2026-09-10T08:00", "2026-09-10T08:15", "bob's quarter"))
	notBillable := entryForm(website, "2026-09-11T08:00", "2026-09-11T10:00", "internal")
	notBillable.Set("billable", "no")
	alice.createEntry(notBillable)
	alice.createEntry(entryForm("", "2026-09-12T08:00", "2026-09-12T10:00", "unassigned"))
	alice.createEntry(entryForm(intranet, "2026-09-13T08:00", "2026-09-13T10:00", "other customer"))
	alice.createEntry(entryForm(hosting, "2026-09-14T08:00", "2026-09-14T08:30", "server"))
	alice.postForm(alice.projectPath(alice.customerPath("acme"), "hosting")+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	draft := alice.createDraft(pullForm(alice, "project", "0"))

	res := alice.get(draft)
	wantInOrder(t, res.body, ">hosting<", "0.5", "h", "100.00", "50.00", ">website<", "1.75", "h", "85.00", "148.75", "198.75")
	res.wantNoBody(t, "server")

	again := alice.createDraft(pullForm(alice, "project", "0"))
	alice.get(again).wantBody(t, "No lines yet.")
}

func TestPerEntryLinesShowTheDescriptionAndDateAndRoundEachLine(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00:00", "2026-09-02T08:07:30", "half up"))
	c.createEntry(entryForm(website, "2026-09-03T08:00:00", "2026-09-03T08:07:29", "rounded away"))
	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T08:20", ""))

	draft := c.createDraft(pullForm(c, "entry", "15"))

	res := c.get(draft)
	wantInOrder(t, res.body, ">half up<", "2026-09-02", "0.25", ">rounded away<", "2026-09-03", "0.00", ">website<", "2026-09-04", "0.25")
}

func TestPerProjectLinesRoundTheirTotalTime(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T08:08", "a"))
	c.createEntry(entryForm(website, "2026-09-03T08:00", "2026-09-03T08:08", "b"))

	wantInOrder(t, c.get(c.createDraft(pullForm(c, "project", "15"))).body, "website", "0.25", "h", "Net")
}

func TestUnroundedHoursRoundHalfUpToTwoDecimals(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00:00", "2026-09-02T09:00:18", "a"))

	wantInOrder(t, c.get(c.createDraft(pullForm(c, "project", "0"))).body, "website", "1.01", "h", "Net")
}

const lockedMessage = "remove it from the draft or cancel the line first"

func TestClaimedEntriesLinkToTheirDraftAndCannotChange(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	entry := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	draft := c.createDraft(pullForm(c, "project", "0"))
	badge := `href="` + draft + `"`

	c.get("/entries?day=2026-09-02").wantBody(t, badge)
	c.get("/week?week=2026-09-02").wantBody(t, badge)
	view := c.get(entry)
	view.wantBody(t, badge)
	view.wantBody(t, "<fieldset disabled>")
	view.wantNoBody(t, entry+"/delete")
	edit := c.postForm(entry, entryForm(website, "2026-09-02T08:00", "2026-09-02T10:00", "more"))
	edit.wantStatus(t, http.StatusConflict)
	edit.wantBody(t, lockedMessage)
	del := c.postForm(entry+"/delete", nil)
	del.wantStatus(t, http.StatusConflict)
	del.wantBody(t, lockedMessage)
	c.get("/entries?day=2026-09-02").wantBody(t, "08:00–09:00")
}

func TestDeletingAGeneratedLineReleasesItsEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	entry := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	draft := c.createDraft(pullForm(c, "project", "0"))

	wantRedirect(t, c.postForm(c.linePath(draft, "website")+"/delete", nil), draft)

	c.get("/entries?day=2026-09-02").wantNoBody(t, `href="`+draft+`"`)
	wantRedirect(t, c.postForm(entry, entryForm(website, "2026-09-02T08:00", "2026-09-02T10:00", "more")), "/entries?day=2026-09-02")
	c.get(c.createDraft(pullForm(c, "project", "0"))).wantBody(t, ">website<")
}

func TestDeletingADraftReleasesItsEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	entry := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	draft := c.createDraft(pullForm(c, "project", "0"))

	wantRedirect(t, c.postForm(draft+"/delete", nil), "/invoices")

	wantRedirect(t, c.postForm(entry+"/delete", nil), "/entries?day=2026-09-02")
}

func TestGeneratedLinesAreEditableAndListTheirEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, _ := alice.acmeProjects()
	alice.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	bob.createEntry(entryForm(website, "2026-09-03T08:00", "2026-09-03T08:30", "review"))
	draft := alice.createDraft(pullForm(alice, "project", "0"))
	line := alice.linePath(draft, "website")

	wantInOrder(t, alice.get(line).body, `value="website"`, "2026-09-02", "alice", "1:00", "design", "2026-09-03", "bob", "0:30", "review")
	form := lineForm("Website relaunch", "2", "90")
	form.Set("detail", "September")
	wantRedirect(t, alice.postForm(line, form), draft)

	wantInOrder(t, alice.get(draft).body, "Website relaunch", "from time", "September", "2", "90.00", "180.00")
}

func TestRefreshRegeneratesGeneratedLinesFromTheCurrentRangeAndKeepsFreeLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "september"))
	draft := c.createDraft(pullForm(c, "project", "0"))
	c.addLine(draft, lineForm("Flat fee", "1", "500"))
	wantRedirect(t, c.postForm(c.linePath(draft, "website"), lineForm("Edited", "9", "1")), draft)
	c.createEntry(entryForm(website, "2026-09-03T08:00", "2026-09-03T08:30", "late september"))
	c.createEntry(entryForm(hosting, "2026-10-01T08:00", "2026-10-01T09:00", "october"))
	wantRedirect(t, c.postForm(draft, url.Values{
		"from": {"2026-09-01"}, "to": {"2026-10-31"},
		"service_from": {"2026-09-01"}, "service_to": {"2026-10-31"},
		"payment_days": {"14"},
	}), draft)

	c.get(draft).wantBody(t, `href="`+draft+`/refresh"`)
	confirm := c.get(draft + "/refresh")
	confirm.wantStatus(t, http.StatusOK)
	confirm.wantBody(t, "Changes to those lines are lost")
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res := c.get(draft)
	res.wantNoBody(t, "Edited")
	wantInOrder(t, res.body, "Flat fee", "500.00", ">hosting<", "1", "100.00", ">website<", "1.5", "85.00", "127.50")
}

func TestDraftWithoutARangePullsNoEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "september"))

	form := pullForm(c, "project", "0")
	form.Set("from", "")
	form.Set("to", "")
	draft := c.createDraft(form)

	res := c.get(draft)
	res.wantBody(t, "No lines yet.")
	res.wantBody(t, `name="from" value=""`)
	c.get("/").in(t, "Drafts").wantRow(t, "acme", "Without time entries")
	c.get(draft+"/refresh").wantBody(t, "Without time entries")
	c.linePath(c.createDraft(pullForm(c, "project", "0")), "website")
}

func TestRefreshWithoutARangeReleasesTheEntriesAndKeepsFreeLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "september"))
	draft := c.createDraft(pullForm(c, "project", "0"))
	c.addLine(draft, lineForm("Flat fee", "1", "500"))
	wantRedirect(t, c.postForm(draft, url.Values{"from": {""}, "to": {""}, "payment_days": {"14"}}), draft)

	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res := c.get(draft)
	res.wantBody(t, "Flat fee")
	res.wantNoBody(t, "from time")
	c.linePath(c.createDraft(pullForm(c, "project", "0")), "website")
}

func TestDraftWithEntriesShowsTheServicePeriodFromThemAndKeepsTheOneSetByHand(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(website, "2026-09-19T08:00", "2026-09-19T09:00", "review"))
	form := pullForm(c, "project", "0")
	form.Set("from", "")
	form.Set("to", "")
	draft := c.createDraft(form)
	byHand := url.Values{"from": {""}, "to": {""}, "service_from": {"2026-09-01"}, "service_to": {"2026-09-05"}, "payment_days": {"14"}}
	wantRedirect(t, c.postForm(draft, byHand), draft)
	byHand.Set("from", "2026-09-01")
	byHand.Set("to", "2026-09-30")
	wantRedirect(t, c.postForm(draft, byHand), draft)
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res := c.get(draft)
	res.wantBody(t, "Service period 2026-09-02 – 2026-09-19")
	res.wantNoBody(t, `name="service_from"`)
	ignored := url.Values{"from": {""}, "to": {""}, "service_from": {"2026-08-01"}, "service_to": {"2026-08-31"}, "payment_days": {"14"}}
	wantRedirect(t, c.postForm(draft, ignored), draft)
	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	res = c.get(draft)
	res.wantBody(t, `name="service_from" value="2026-09-01"`)
	res.wantBody(t, `name="service_to" value="2026-09-05"`)
}

func TestRefreshingAnotherDraftCannotTakeClaimedEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "september"))
	first := c.createDraft(pullForm(c, "project", "0"))
	second := c.createDraft(pullForm(c, "project", "0"))

	wantRedirect(t, c.postForm(second+"/refresh", nil), second)

	c.get(second).wantBody(t, "No lines yet.")
	c.get(first).wantBody(t, ">website<")
	c.postForm("/invoices/999/refresh", nil).wantStatus(t, http.StatusNotFound)
}

func TestRunningTimersAreNotPulled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	c.startTimer(url.Values{"project": {website}})
	ta.clock.Advance(time.Hour)

	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))

	c.get(draft).wantBody(t, "No lines yet.")
}
