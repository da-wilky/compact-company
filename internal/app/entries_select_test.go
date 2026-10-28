package app_test

import (
	"fmt"
	"html"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

var selectBox = regexp.MustCompile(`<input type="checkbox" name="id" value="(\d+)"`)

// selectable lists the entry paths the page offers to select.
func (r *response) selectable() []string {
	var paths []string
	for _, m := range selectBox.FindAllStringSubmatch(r.body, -1) {
		paths = append(paths, "/entries/"+m[1])
	}
	return paths
}

func wantPaths(t *testing.T, got []string, want ...string) {
	t.Helper()
	if strings.Join(got, " ") != strings.Join(want, " ") {
		t.Fatalf("paths = %q, want %q", got, want)
	}
}

// deleteEntries posts the selection as the entries list does: to the list's own query.
func (c *client) deleteEntries(query string, paths ...string) *response {
	c.t.Helper()
	form := url.Values{}
	for _, p := range paths {
		form.Add("id", strings.TrimPrefix(p, "/entries/"))
	}
	return c.hxPost("/entries/delete"+query, form)
}

func TestOnlyOwnUnclaimedEndedEntriesAreSelectable(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, _ := alice.acmeProjects()
	alice.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	alice.createDraft(pullForm(alice, "project", "0"))
	free := alice.createEntry(entryForm("", "2026-09-03T08:00", "2026-09-03T09:00", "free"))
	bob.createEntry(entryForm("", "2026-09-03T10:00", "2026-09-03T11:00", "bob's"))
	alice.startTimer(url.Values{})

	page := alice.get("/entries?everyone=1")

	page.wantBody(t, "billed")
	page.wantBody(t, "bob&#39;s")
	wantPaths(t, page.selectable(), free)
}

func TestSelectedEntriesAreDeletedTogetherKeepingFiltersAndPage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	var paths []string
	for _, h := range []string{"08", "09", "10", "11"} {
		paths = append(paths, c.createEntry(entryForm(project, "2026-10-26T"+h+":00", "2026-10-26T"+h+":30", "slot "+h)))
	}
	c.createEntry(entryForm("", "2026-10-26T12:00", "2026-10-26T12:30", "elsewhere"))
	query := "?day=2026-10-26&project=" + project
	if got := c.get("/entries" + query).deleteAction(t); got != "/entries/delete"+query {
		t.Fatalf("selection posts to %q", got)
	}

	res := c.deleteEntries(query, paths[0], paths[2])

	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.text(), "Entries on 2026-10-26", "acme · website", "slot 11", "slot 09")
	res.wantNoBody(t, "slot 08")
	res.wantNoBody(t, "slot 10")
	res.wantNoBody(t, "elsewhere")
	c.get(paths[0]).wantStatus(t, http.StatusNotFound)
	c.get(paths[1]).wantStatus(t, http.StatusOK)
}

func TestTheSelectionPostsToTheListsPageAndPageSize(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for i := range 26 {
		m := i * 50
		c.createEntry(entryForm("", fmt.Sprintf("2026-10-26T%02d:%02d", m/60, m%60), fmt.Sprintf("2026-10-26T%02d:%02d", (m+45)/60, (m+45)%60), fmt.Sprintf("entry %02d", i)))
	}
	next := html.UnescapeString(moreLink.FindStringSubmatch(c.get("/entries?everyone=1&per_page=25").body)[1])

	if got, want := c.get(next).deleteAction(t), "/entries/delete"+strings.TrimPrefix(next, "/entries"); got != want {
		t.Fatalf("selection posts to %q, want %q", got, want)
	}
	if !strings.Contains(next, "before=") {
		t.Fatalf("next page %q has no cursor", next)
	}
}

func TestSelectionWithAnUndeletableEntryDeletesNothingAndNamesTheBlockers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, _ := alice.acmeProjects()
	claimed := alice.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	alice.createDraft(pullForm(alice, "project", "0"))
	free := alice.createEntry(entryForm("", "2026-09-03T08:00", "2026-09-03T09:00", "free"))
	bobs := bob.createEntry(entryForm("", "2026-09-03T10:00", "2026-09-03T11:00", "bob's"))

	res := alice.deleteEntries("?everyone=1", free, claimed, bobs)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was deleted. These entries can't be deleted:",
		"2026-09-02 08:00 “billed” · On a draft", "2026-09-03 10:00 “bob's” · entry of bob")
	wantPaths(t, res.selectable(), free)
	res.wantBody(t, `value="`+strings.TrimPrefix(free, "/entries/")+`" form="entry-selection" aria-label="Select the entry of 2026-09-03 08:00" checked>`)
	alice.get(free).wantStatus(t, http.StatusOK)
	alice.get(claimed).wantStatus(t, http.StatusOK)
	bob.get(bobs).wantStatus(t, http.StatusOK)
}

func TestSelectionWithARunningTimerDeletesNothing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	free := c.createEntry(entryForm("", "2026-10-26T07:00", "2026-10-26T08:00", "free"))
	timer := c.startTimer(url.Values{"description": {"tracking"}})

	res := c.deleteEntries("", free, "/entries/"+strings.TrimPrefix(timer, "/timers/"))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "These entries can't be deleted:", "2026-10-26 09:00 “tracking” · running")
	c.get(free).wantStatus(t, http.StatusOK)
}

// selectionQuestion is what the confirmation of the action confirmed with ok asks with n entries selected;
// app.js fills in the count.
func (r *response) selectionQuestion(t *testing.T, ok string, n int) string {
	t.Helper()
	m := regexp.MustCompile(`data-confirm-one="([^"]*)" data-confirm-many="([^"]*)" data-confirm-ok="` + regexp.QuoteMeta(ok) + `"`).FindStringSubmatch(r.body)
	if m == nil {
		t.Fatalf("selection asks no confirmation for %q:\n%s", ok, r.body)
	}
	if n == 1 {
		return html.UnescapeString(m[1])
	}
	return strings.ReplaceAll(html.UnescapeString(m[2]), "{n}", fmt.Sprint(n))
}

func TestDeletingTheSelectionAsksForConfirmationNamingTheCount(t *testing.T) {
	en := newTestApp(t, devConfig).client()
	en.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))
	de := newTestApp(t, devConfig+"[defaults]\nlanguage = \"de\"\n").client()
	de.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))

	page, seite := en.get("/entries"), de.get("/entries")

	wantConfirmation(t, page.selectionQuestion(t, "Delete", 7), "Delete 7 entries?")
	wantConfirmation(t, page.selectionQuestion(t, "Delete", 1), "Delete 1 entry?")
	wantConfirmation(t, seite.selectionQuestion(t, "Löschen", 7), "7 Einträge löschen?")
	wantConfirmation(t, seite.selectionQuestion(t, "Löschen", 1), "1 Eintrag löschen?")
}

var selectionForm = regexp.MustCompile(`<form id="entry-selection"[^>]* action="([^"]*)"`)

func (r *response) deleteAction(t *testing.T) string {
	t.Helper()
	m := selectionForm.FindStringSubmatch(r.body)
	if m == nil {
		t.Fatalf("no selection form:\n%s", r.body)
	}
	return html.UnescapeString(m[1])
}

// massEdit posts the selection with extra fields to action (assign, billable), as the toolbar's buttons do.
func (c *client) massEdit(action, query string, form url.Values, paths ...string) *response {
	c.t.Helper()
	for _, p := range paths {
		form.Add("id", strings.TrimPrefix(p, "/entries/"))
	}
	return c.hxPost("/entries/"+action+query, form)
}

func TestSelectedEntriesAreAssignedToAProjectTogether(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	explicit := entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "explicit")
	explicit.Set("billable", "no")
	first := c.createEntry(explicit)
	second := c.createEntry(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "orphan"))
	c.createEntry(entryForm("", "2026-10-26T10:00", "2026-10-26T11:00", "left alone"))

	res := c.massEdit("assign", "?day=2026-10-26", url.Values{"project": {hosting}}, first, second)

	res.wantStatus(t, http.StatusOK)
	res.wantRow(t, "", "08:00–09:00", "1:00", "acme · hosting", "explicit", "no")
	res.wantRow(t, "", "09:00–10:00", "1:00", "acme · hosting", "orphan", "yes")
	res.wantRow(t, "", "10:00–11:00", "1:00", "Unassigned", "left alone", "")
}

func TestAssigningOffersOnlyProjectsThatReceiveTimeAndAsksNamingTheCount(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.postForm("/projects/"+hosting+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))

	page := c.get("/entries")

	page.wantBody(t, `<optgroup label="acme"><option value="`+website+`">website</option></optgroup>`)
	page.wantNoBody(t, `value="`+hosting+`"`)
	wantConfirmation(t, page.selectionQuestion(t, "Assign", 3), "Assign 3 entries to the chosen project?")
	wantConfirmation(t, page.selectionQuestion(t, "Assign", 1), "Assign 1 entry to the chosen project?")
}

func TestAssigningToAnArchivedProjectChangesNothing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	_, hosting := c.acmeProjects()
	c.postForm("/projects/"+hosting+"/archive", nil).wantStatus(t, http.StatusSeeOther)
	orphan := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", "orphan"))

	res := c.massEdit("assign", "", url.Values{"project": {hosting}}, orphan)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project can&#39;t receive new time.")
	wantPaths(t, res.selectable(), orphan)
	res.wantBody(t, `aria-label="Select the entry of 2026-10-26 08:00" checked>`)
	res.wantRow(t, "", "08:00–09:00", "1:00", "Unassigned", "orphan", "")
}

func TestSelectionWithAnUnchangeableEntryAssignsNothingAndNamesTheBlockers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, hosting := alice.acmeProjects()
	claimed := alice.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "billed"))
	alice.createDraft(pullForm(alice, "project", "0"))
	free := alice.createEntry(entryForm("", "2026-09-03T08:00", "2026-09-03T09:00", "free"))
	bobs := bob.createEntry(entryForm("", "2026-09-03T10:00", "2026-09-03T11:00", "bob's"))

	res := alice.massEdit("assign", "?everyone=1", url.Values{"project": {hosting}}, free, claimed, bobs)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was changed. These entries can't be changed:",
		"2026-09-02 08:00 “billed” · On a draft", "2026-09-03 10:00 “bob's” · entry of bob")
	wantPaths(t, res.selectable(), free)
	res.wantRow(t, "", "alice", "08:00–09:00", "1:00", "Unassigned", "free", "")
	res.wantRow(t, "", "bob", "10:00–11:00", "1:00", "Unassigned", "bob's", "")
}

func TestSelectedEntriesAreMarkedBillableOrNotTogether(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, _ := c.acmeProjects()
	off := entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "first")
	off.Set("billable", "no")
	first := c.createEntry(off)
	second := c.createEntry(entryForm(website, "2026-10-26T09:00", "2026-10-26T10:00", "second"))
	c.createEntry(entryForm(website, "2026-10-26T10:00", "2026-10-26T11:00", "left alone"))

	billable := c.massEdit("billable", "?day=2026-10-26", url.Values{"billable": {"yes"}}, first, second)

	billable.wantStatus(t, http.StatusOK)
	billable.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "first", "yes")
	billable.wantRow(t, "", "09:00–10:00", "1:00", "acme · website", "second", "yes")

	notBillable := c.massEdit("billable", "?day=2026-10-26", url.Values{"billable": {"no"}}, first, second)

	notBillable.wantStatus(t, http.StatusOK)
	notBillable.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "first", "no")
	notBillable.wantRow(t, "", "09:00–10:00", "1:00", "acme · website", "second", "no")
	notBillable.wantRow(t, "", "10:00–11:00", "1:00", "acme · website", "left alone", "yes")
}

func TestSelectionWithAnUnassignedEntryChangesNoBillableFlag(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websiteProject()
	assigned := c.createEntry(entryForm(website, "2026-10-26T08:00", "2026-10-26T09:00", "assigned"))
	orphan := c.createEntry(entryForm("", "2026-10-26T09:00", "2026-10-26T10:00", "orphan"))

	res := c.massEdit("billable", "", url.Values{"billable": {"no"}}, assigned, orphan)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was changed. These entries can't be changed:", "2026-10-26 09:00 “orphan” · no project")
	res.wantRow(t, "", "08:00–09:00", "1:00", "acme · website", "assigned", "yes")
}

func TestMarkingTheSelectionAsksNamingTheCount(t *testing.T) {
	en := newTestApp(t, devConfig).client()
	en.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))
	de := newTestApp(t, devConfig+"[defaults]\nlanguage = \"de\"\n").client()
	de.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))

	page, seite := en.get("/entries"), de.get("/entries")

	wantConfirmation(t, page.selectionQuestion(t, "Billable", 4), "Mark 4 entries billable?")
	wantConfirmation(t, page.selectionQuestion(t, "Not billable", 1), "Mark 1 entry not billable?")
	wantConfirmation(t, seite.selectionQuestion(t, "Abrechenbar", 4), "4 Einträge als abrechenbar markieren?")
	wantConfirmation(t, seite.selectionQuestion(t, "Nicht abrechenbar", 1), "1 Eintrag als nicht abrechenbar markieren?")
}
