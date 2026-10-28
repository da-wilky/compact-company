package app_test

import (
	"net/http"
	"net/url"
	"slices"
	"strings"
	"testing"
)

// lineRows are the rows of an invoice's lines table between its header and its totals or "Add line".
func (r *response) lineRows(t *testing.T) [][]string {
	t.Helper()
	rows := r.rows()
	end := slices.IndexFunc(rows, func(row []string) bool { return row[0] == "Add line" || row[0] == "Net" })
	if len(rows) == 0 || end < 0 {
		t.Fatalf("no lines table:\n%s", r.body)
	}
	return rows[1:end]
}

func wantRows(t *testing.T, got [][]string, want ...[]string) {
	t.Helper()
	if !slices.EqualFunc(got, want, slices.Equal) {
		t.Fatalf("rows:\n%q\nwant:\n%q", got, want)
	}
}

func TestProjectGroupingShowsAWorkingTimeLineUnderEachProjectsHeadingAndFreeLinesLast(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "a"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "b"))
	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T08:45", "c"))
	draft := c.createDraft(pullForm(c, "project", "0"))
	c.addLine(draft, lineForm("Consulting flat fee", "1", "200"))

	wantRows(t, c.get(draft).lineRows(t),
		[]string{"hosting"},
		[]string{"Working time from time", "0.5", "h", "100.00", "50.00", ""},
		[]string{"website"},
		[]string{"Working time from time", "1.75", "h", "85.00", "148.75", ""},
		[]string{"Consulting flat fee", "1", "h", "200.00", "200.00", ""},
	)
}

func TestWorkingTimeIsInTheInvoiceLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	form := customerForm("acme", "85")
	form.Set("invoice_language", "de")
	c.createCustomer(form)
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	c.createEntry(entryForm(strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"), "2026-09-02T08:00", "2026-09-02T09:00", "a"))

	draft := c.createDraft(pullForm(c, "project", "0"))

	wantRows(t, c.get(draft).lineRows(t),
		[]string{"website"},
		[]string{"Arbeitszeit from time", "1", "h", "85.00", "85.00", ""},
	)
}

func TestLinesMoveOnlyWithinTheirSection(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T09:00", "launch"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, lineForm("Fee", "1", "10"))

	for _, move := range []string{"server/down", "design/up", "Fee/up", "design/down"} {
		line, dir, _ := strings.Cut(move, "/")
		wantRedirect(t, c.postForm(c.linePath(draft, line)+"/"+dir, nil), draft)
	}

	wantInOrder(t, c.get(draft).text(), "hosting server", "website launch", "design", "Fee")
}

func TestEachLineCarriesItsProjectToTheTemplateInSectionOrder(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, lineForm("Fee", "1", "10"))

	var got [][2]any
	for _, l := range c.get(draft + "/preview").templateInput(t).(map[string]any)["lines"].([]any) {
		line := l.(map[string]any)
		got = append(got, [2]any{line["project"], line["description"]})
	}

	if want := [][2]any{{"hosting", "server"}, {"website", "design"}, {nil, "Fee"}}; !slices.Equal(got, want) {
		t.Fatalf("lines (project, description) = %v, want %v", got, want)
	}
}

func TestIssuedAndCancellationInvoicesShowTheProjectSectionsFrozenAtIssuing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	inv := c.createAndIssue(pullForm(c, "project", "0"), lineForm("Fee", "1", "10"))
	wantRedirect(t, c.postForm("/projects/"+website, projectForm("renamed", "")), c.customerPath("acme")+"/projects")

	confirm := c.get(inv + "/cancel")
	wantInOrder(t, confirm.text(), "hosting Working time", "website Working time", "Fee")
	ids := cancelCheckbox.FindAllStringSubmatch(confirm.body, -1)
	res := c.postForm(inv+"/cancel", url.Values{"line": {ids[1][1], ids[2][1]}})
	res.wantStatus(t, http.StatusSeeOther)
	cancellation := res.Header.Get("Location")

	wantRows(t, c.get(inv).lineRows(t),
		[]string{"hosting"},
		[]string{"Working time", "0.5", "h", "100.00", "50.00", ""},
		[]string{"website"},
		[]string{"Working time", "1", "h", "85.00", "85.00", "Cancelled by 2026-002"},
		[]string{"Fee", "1", "h", "10.00", "10.00", "Cancelled by 2026-002"},
	)
	wantRows(t, c.get(cancellation).lineRows(t),
		[]string{"renamed"},
		[]string{"Working time", "-1", "h", "85.00", "-85.00", ""},
		[]string{"Fee", "-1", "h", "10.00", "-10.00", ""},
	)
}

func TestEntryGroupingOrdersLinesByProjectThenDateUnderTheirHeadings(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T09:00", "launch"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))

	draft := c.createDraft(pullForm(c, "entry", "0"))

	wantRows(t, c.get(draft).lineRows(t),
		[]string{"hosting"},
		[]string{"server from time 2026-09-03", "0.5", "h", "100.00", "50.00", ""},
		[]string{"website"},
		[]string{"design from time 2026-09-02", "1", "h", "85.00", "85.00", ""},
		[]string{"launch from time 2026-09-04", "1", "h", "85.00", "85.00", ""},
	)
}
