package app_test

import (
	"fmt"
	"html"
	"net/http"
	"net/http/httptest"
	"net/url"
	"regexp"
	"strings"
	"testing"
	"time"
)

func entryForm(project, start, end, description string) url.Values {
	return url.Values{"project": {project}, "start": {start}, "end": {end}, "description": {description}, "billable": {""}}
}

func TestManualEntryShowsInTheDayListing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	res := c.postForm("/entries", entryForm(project, "2026-10-26T08:00", "2026-10-26T09:45", "kickoff"))

	wantRedirect(t, res, "/entries?day=2026-10-26")
	list := c.get("/entries?day=2026-10-26")
	list.wantStatus(t, http.StatusOK)
	list.wantBody(t, "acme · website")
	list.wantBody(t, "kickoff")
	list.wantBody(t, "08:00–09:45")
	list.wantRow(t, "08:00–09:45", "1:45", "acme · website", "kickoff", "yes")
	c.get("/entries?day=2026-10-27").wantNoBody(t, "kickoff")
}

func TestEntriesListsAllDaysNewestFirstGroupedByDay(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createEntry(entryForm("", "2026-10-20T08:00", "2026-10-20T09:00", "last week"))
	c.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T06:30", "early"))
	c.createEntry(entryForm("", "2026-10-26T07:00", "2026-10-26T08:00", "later"))

	res := c.get("/entries")

	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.text(), "Mon 2026-10-26 1:30", "later", "early", "Tue 2026-10-20 1:00", "last week")
}

func (c *client) hxGet(path string) *response {
	c.t.Helper()
	req := httptest.NewRequest(http.MethodGet, baseURL+path, nil)
	req.Header.Set("HX-Request", "true")
	return c.do(req)
}

var moreLink = regexp.MustCompile(`<a class="more" href="([^"]+)"`)

func TestEntriesLoadTenAtATimeAndContinueTheDay(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for h := 8; h < 19; h++ {
		c.createEntry(entryForm("", fmt.Sprintf("2026-10-26T%02d:00", h), fmt.Sprintf("2026-10-26T%02d:30", h), fmt.Sprintf("entry %02d", h)))
	}
	c.createEntry(entryForm("", "2026-10-25T08:00", "2026-10-25T09:00", "sunday"))

	first := c.get("/entries")

	wantInOrder(t, first.text(), "Mon 2026-10-26 5:30")
	wantInOrder(t, first.body, "entry 18", "entry 09")
	first.wantNoBody(t, "entry 08")
	m := moreLink.FindStringSubmatch(first.body)
	if m == nil {
		t.Fatalf("no load-more link:\n%s", first.body)
	}
	more := c.hxGet(html.UnescapeString(m[1]))
	more.wantNoBody(t, "<html")
	more.wantNoBody(t, "entry 09")
	more.wantNoBody(t, "Mon 2026-10-26")
	wantInOrder(t, more.text(), "entry 08", "Sun 2026-10-25 1:00", "sunday")
	more.wantNoBody(t, "Load more")
	c.get("/entries?before=nope").wantStatus(t, http.StatusBadRequest)
}

func TestEntryFiltersShowTheProjectAndKeepEachOther(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	res := c.get("/entries?day=2026-10-26&project=" + project)

	res.wantBody(t, `acme · website <a href="/entries?day=2026-10-26"`)
	res.wantBody(t, `href="/entries?day=2026-10-26&amp;everyone=1&amp;project=`+project+`">Team</a>`)
	res.wantBody(t, `href="/entries?project=`+project+`">All entries</a>`)
	res.wantNoBody(t, ">Today</a>")
	c.get("/entries?day=2026-10-25").wantBody(t, `href="/entries?day=2026-10-26">Today</a>`)
	c.get("/entries?project=999").wantStatus(t, http.StatusBadRequest)
}

// wantInOrder fails unless each of parts occurs in body, after the one before it.
func wantInOrder(t *testing.T, body string, parts ...string) {
	t.Helper()
	rest := body
	for _, p := range parts {
		i := strings.Index(rest, p)
		if i < 0 {
			t.Fatalf("body lacks %q after the previous parts:\n%s", p, body)
		}
		rest = rest[i+len(p):]
	}
}

var entryLink = regexp.MustCompile(`href="(/entries/\d+)[?"]`)

// createEntry creates an entry and returns its path, scraped from its day's listing (newest first).
func (c *client) createEntry(form url.Values) string {
	c.t.Helper()
	res := c.postForm("/entries", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	m := entryLink.FindAllStringSubmatch(c.get(res.Header.Get("Location")).body, -1)
	if m == nil {
		c.t.Fatal("no entry link in listing")
	}
	return m[0][1]
}

func TestAssigningAnUnassignedEntryAppliesTheProjectDefault(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	entry := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "orphan"))

	form := c.get(entry)
	form.wantStatus(t, http.StatusOK)
	form.wantBody(t, `value="2026-10-26T08:00"`)
	form.wantBody(t, `value="2026-10-26T09:00"`)
	form.wantBody(t, `value="orphan"`)
	res := c.postForm(entry, entryForm(project, "2026-10-26T08:00", "2026-10-26T09:30", "adopted"))

	wantRedirect(t, res, "/entries?day=2026-10-26")
	list := c.get("/entries?day=2026-10-26")
	list.wantBody(t, "08:00–09:30")
	list.wantRow(t, "08:00–09:30", "1:30", "acme · website", "adopted", "yes")
	c.get(entry).wantBody(t, `<option value="yes" selected>`)
}

func TestOwnerCanFullyEditAnEntry(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	entry := c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "draft"))
	other := c.createEntry(entryForm("", "2026-10-27T08:00", "2026-10-27T09:00", "tuesday"))

	edit := entryForm("", "2026-10-27T10:00", "2026-10-27T10:20", "moved")
	res := c.postForm(entry, edit)

	wantRedirect(t, res, "/entries?day=2026-10-27")
	c.get("/entries?day=2026-10-26").wantBody(t, "No entries.")
	c.get("/entries?day=2026-10-27").wantRow(t, "10:00–10:20", "0:20", "Unassigned", "moved", "")
	clash := c.postForm(entry, entryForm("", "2026-10-27T08:30", "2026-10-27T10:20", "moved"))
	clash.wantStatus(t, http.StatusUnprocessableEntity)
	clash.wantBody(t, "This entry overlaps another of your entries.")
	c.postForm(other, entryForm("", "2026-10-27T08:00", "2026-10-27T09:30", "longer")).wantStatus(t, http.StatusSeeOther)
}

func TestOwnerCanDeleteAnEntry(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	entry := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "mistake"))

	res := c.postForm(entry+"/delete", nil)

	wantRedirect(t, res, "/entries?day=2026-10-26")
	c.get("/entries?day=2026-10-26").wantNoBody(t, "mistake")
	c.get(entry).wantStatus(t, http.StatusNotFound)
	c.postForm(entry+"/delete", nil).wantStatus(t, http.StatusNotFound)
}

func TestColleaguesCanViewButNotChangeAnEntry(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	entry := alice.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "alice work"))

	view := bob.get(entry)
	view.wantStatus(t, http.StatusOK)
	view.wantBody(t, `value="alice work"`)
	view.wantBody(t, "<fieldset disabled>")
	view.wantNoBody(t, "<button>Save</button>")
	view.wantNoBody(t, entry+"/delete")
	bob.postForm(entry, entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "hijacked")).wantStatus(t, http.StatusForbidden)
	bob.postForm(entry+"/delete", nil).wantStatus(t, http.StatusForbidden)

	alice.get("/entries?day=2026-10-26").wantBody(t, "alice work")
	bob.get("/entries?day=2026-10-26").wantNoBody(t, "alice work")
	everyone := bob.get("/entries?day=2026-10-26&everyone=1")
	everyone.wantRow(t, "alice", "08:00–09:00", "1:00", "Unassigned", "alice work", "")
	everyone.wantNoBody(t, `href="`+entry+`"`)
	everyone.wantBody(t, `href="/entries?day=2026-10-27&amp;everyone=1"`)
}

func TestDayListingFiltersByProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "on website"))
	c.createEntry(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "orphan"))

	website := c.get("/entries?day=2026-10-26&project=" + project)
	unassigned := c.get("/entries?day=2026-10-26&project=none")

	website.wantBody(t, "on website")
	website.wantNoBody(t, "orphan")
	website.wantBody(t, `href="/entries?day=2026-10-27&amp;project=`+project+`"`)
	unassigned.wantBody(t, "orphan")
	unassigned.wantNoBody(t, "on website")
	c.get("/entries?day=2026-10-26&project=x").wantStatus(t, http.StatusBadRequest)
}

func TestEntryBillableInheritsTheProjectDefaultUnlessSet(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	internal := projectForm("internal", "")
	internal.Del("billable")
	c.createProject(cust, internal)
	c.createProject(cust, projectForm("website", ""))
	internalID := strings.TrimPrefix(c.projectPath(cust, "internal"), "/projects/")
	websiteID := strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/")

	c.postForm("/entries", entryForm(internalID, "2026-10-26T08:00", "2026-10-26T09:00", "inherited")).wantStatus(t, http.StatusSeeOther)
	explicit := entryForm(websiteID, "2026-10-26T09:00", "2026-10-26T10:00", "explicit")
	explicit.Set("billable", "no")
	c.postForm("/entries", explicit).wantStatus(t, http.StatusSeeOther)
	c.postForm("/entries", entryForm(websiteID, "2026-10-26T10:00", "2026-10-26T11:00", "default")).wantStatus(t, http.StatusSeeOther)
	c.postForm("/entries", entryForm("", "2026-10-26T11:00", "2026-10-26T12:00", "orphan")).wantStatus(t, http.StatusSeeOther)

	list := c.get("/entries?day=2026-10-26")
	list.wantRow(t, "08:00–09:00", "1:00", "acme · internal", "inherited", "no")
	list.wantRow(t, "09:00–10:00", "1:00", "acme · website", "explicit", "no")
	list.wantRow(t, "10:00–11:00", "1:00", "acme · website", "default", "yes")
	list.wantRow(t, "11:00–12:00", "1:00", "Unassigned", "orphan", "")
}

func TestUnassignedEntryCannotBeMarkedBillable(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	form := entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "orphan")
	form.Set("billable", "yes")

	res := c.postForm("/entries", form)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "An entry without a project can&#39;t be billable")
	res.wantBody(t, `value="orphan"`)
	c.get("/entries?day=2026-10-26").wantNoBody(t, "orphan")
}

func TestEntryEndingBeforeItStartsIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.postForm("/entries", entryForm("", "2026-10-26T10:00", "2026-10-26T09:59", "backwards"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "End must not be before start.")
	res.wantBody(t, `value="2026-10-26T10:00"`)
	c.postForm("/entries", entryForm("", "2026-10-26T10:00", "", "")).wantBody(t, "End must be a date and time.")
	c.get("/entries?day=2026-10-26").wantBody(t, "No entries.")
}

func TestOverlappingEntriesAreRefusedWithParallelEntriesOff(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	alice.postForm("/entries", entryForm("", "2026-10-26T06:00", "2026-10-26T07:00", "early")).wantStatus(t, http.StatusSeeOther)

	res := alice.postForm("/entries", entryForm("", "2026-10-26T06:30", "2026-10-26T07:30", "clash"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This entry overlaps another of your entries.")
	alice.postForm("/entries", entryForm("", "2026-10-26T07:00", "2026-10-26T08:00", "adjacent")).wantStatus(t, http.StatusSeeOther)
	bob.postForm("/entries", entryForm("", "2026-10-26T06:30", "2026-10-26T07:30", "bob")).wantStatus(t, http.StatusSeeOther)
	alice.startTimer(url.Values{})
	alice.postForm("/entries", entryForm("", "2026-10-26T08:30", "2026-10-26T09:30", "into timer")).
		wantBody(t, "This entry overlaps another of your entries.")
	alice.postForm("/entries", entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "before timer")).wantStatus(t, http.StatusSeeOther)
}

func TestOverlappingEntriesAreAllowedWithParallelEntriesOn(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()
	c.postForm("/entries", entryForm("", "2026-10-26T06:00", "2026-10-26T07:00", "one")).wantStatus(t, http.StatusSeeOther)

	c.postForm("/entries", entryForm("", "2026-10-26T06:30", "2026-10-26T07:30", "two")).wantStatus(t, http.StatusSeeOther)
}

func TestStoppedTimerIsListedWithItsEndAndDuration(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	timer := c.startTimer(url.Values{"description": {"tracked"}})
	ta.clock.Advance(90*time.Minute + 30*time.Second)

	c.get("/entries").wantBody(t, "09:00–running")
	c.hxPost(timer+"/stop", nil)

	list := c.get("/entries")
	wantInOrder(t, list.text(), "Mon 2026-10-26 1:30")
	list.wantBody(t, "09:00–10:30")
	list.wantRow(t, "09:00–10:30", "1:30", "Unassigned", "tracked", "")
}

func TestEntriesAreEnteredAndAttributedInTheViewersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	c := ta.client()

	res := c.postForm("/entries", entryForm("", "2026-10-27T00:30", "2026-10-27T01:30", "night shift"))

	wantRedirect(t, res, "/entries?day=2026-10-27")
	c.get("/entries?day=2026-10-27").wantBody(t, "00:30–01:30")
	c.postForm("/profile", url.Values{"language": {"en"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)
	utc := c.get("/entries?day=2026-10-26")
	utc.wantBody(t, "23:30–00:30")
	utc.wantBody(t, "night shift")
}
