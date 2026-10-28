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

// selectionQuestion is what the delete confirmation asks with n entries selected; app.js fills in the count.
func (r *response) selectionQuestion(t *testing.T, n int) string {
	t.Helper()
	m := regexp.MustCompile(`data-confirm-one="([^"]*)" data-confirm-many="([^"]*)"`).FindStringSubmatch(r.body)
	if m == nil {
		t.Fatalf("selection asks no confirmation:\n%s", r.body)
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

	wantConfirmation(t, page.selectionQuestion(t, 7), "Delete 7 entries?")
	wantConfirmation(t, page.selectionQuestion(t, 1), "Delete 1 entry?")
	wantConfirmation(t, seite.selectionQuestion(t, 7), "7 Einträge löschen?")
	wantConfirmation(t, seite.selectionQuestion(t, 1), "1 Eintrag löschen?")
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
