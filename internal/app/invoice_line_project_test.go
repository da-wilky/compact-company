package app_test

import (
	"net/http"
	"net/url"
	"slices"
	"strings"
	"testing"
)

func assignedLineForm(description, quantity, unitPrice, project string) url.Values {
	form := lineForm(description, quantity, unitPrice)
	form.Set("project", project)
	return form
}

func projectOption(id string) string {
	return `<option value="` + id + `"`
}

// lineProjects narrows r to the line form's project dropdown, or to nothing without one.
func (r *response) lineProjects() *response {
	_, after, _ := strings.Cut(r.body, `Project <select name="project">`)
	options, _, _ := strings.Cut(after, "</select>")
	return &response{Response: r.Response, body: options}
}

func TestAssignedManualLineFollowsItsProjectsGeneratedLinesAndFreeLinesStayLast(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	draft := c.createDraft(pullForm(c, "entry", "0"))

	c.addLine(draft, lineForm("Fee", "1", "10"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))

	wantRows(t, c.get(draft).lineRows(t),
		[]string{"hosting"},
		[]string{"server from time 2026-09-03", "0.5", "h", "100.00", "50.00", ""},
		[]string{"Domain", "1", "h", "20.00", "20.00", ""},
		[]string{"website"},
		[]string{"design from time 2026-09-02", "1", "h", "85.00", "85.00", ""},
		[]string{"Fee", "1", "h", "10.00", "10.00", ""},
	)
}

func TestLineFormOffersAndAcceptsOnlyTheProjectsTheInvoiceBills(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createProject(c.customerPath("acme"), projectForm("support", ""))
	support := strings.TrimPrefix(c.projectPath(c.customerPath("acme"), "support"), "/projects/")
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("intranet", ""))
	intranet := strings.TrimPrefix(c.projectPath(c.customerPath("globex"), "intranet"), "/projects/")
	form := pullForm(c, "entry", "0")
	form.Del("all_projects")
	form["project"] = []string{website, support}
	chosen := c.createDraft(form)
	all := c.createDraft(pullForm(c, "entry", "0"))

	res := c.get(chosen + "/lines/new").lineProjects()
	wantInOrder(t, res.body, `<option value="">No project</option>`, projectOption(support), projectOption(website))
	res.wantNoBody(t, projectOption(hosting))
	res.wantNoBody(t, projectOption(intranet))
	res = c.get(all + "/lines/new").lineProjects()
	wantInOrder(t, res.body, projectOption(hosting), projectOption(support), projectOption(website))
	res.wantNoBody(t, projectOption(intranet))

	for name, project := range map[string]string{"unchosen": hosting, "foreign": intranet, "garbage": "x"} {
		t.Run(name, func(t *testing.T) {
			res := c.postForm(chosen+"/lines", assignedLineForm("Domain", "1", "20", project))

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, "Choose one of the projects this invoice bills.")
		})
	}
	c.get(chosen).wantNoBody(t, "Domain")
}

func TestManualLineMovedToAnotherProjectGoesToTheEndOfItsSection(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))
	c.addLine(draft, assignedLineForm("Fee", "1", "10", website))
	domain := c.linePath(draft, "Domain")
	c.get(domain).lineProjects().wantBody(t, projectOption(hosting)+` selected`)

	wantRedirect(t, c.postForm(domain, assignedLineForm("Domain", "1", "20", website)), draft)
	wantInOrder(t, c.get(draft).text(), "hosting server", "website design", "Fee", "Domain")

	wantRedirect(t, c.postForm(domain, lineForm("Domain", "1", "20")), draft)
	c.get(domain).lineProjects().wantNoBody(t, ` selected`)
	wantRows(t, c.get(draft).lineRows(t),
		[]string{"hosting"},
		[]string{"server from time 2026-09-03", "0.5", "h", "100.00", "50.00", ""},
		[]string{"website"},
		[]string{"design from time 2026-09-02", "1", "h", "85.00", "85.00", ""},
		[]string{"Fee", "1", "h", "10.00", "10.00", ""},
		[]string{"Domain", "1", "h", "20.00", "20.00", ""},
	)
}

func TestGeneratedLineKeepsItsEntriesProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	design := c.linePath(draft, "design")
	c.get(design).wantNoBody(t, `Project <select`)

	wantRedirect(t, c.postForm(design, assignedLineForm("design", "1", "85", hosting)), draft)

	wantRows(t, c.get(draft).lineRows(t), []string{"website"}, []string{"design from time", "1", "h", "85.00", "85.00", ""})
}

func TestRefreshKeepsAssignedManualLinesAfterTheirProjectsGeneratedLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))
	c.addLine(draft, assignedLineForm("Fee", "1", "10", website))
	c.addLine(draft, lineForm("Travel", "1", "5"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	c.createEntry(entryForm(website, "2026-09-04T08:00", "2026-09-04T09:00", "launch"))

	wantRedirect(t, c.postForm(draft+"/refresh", nil), draft)

	wantInOrder(t, c.get(draft).text(), "hosting server", "Domain", "website design", "launch", "Fee", "Travel")
}

func TestPDFDataCarriesAnAssignedManualLineInItsSection(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, lineForm("Fee", "1", "10"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))

	var got [][2]any
	for _, l := range c.get(draft + "/preview").templateInput(t).(map[string]any)["lines"].([]any) {
		line := l.(map[string]any)
		got = append(got, [2]any{line["project"], line["description"]})
	}

	if want := [][2]any{{"hosting", "server"}, {"hosting", "Domain"}, {"website", "design"}, {nil, "Fee"}}; !slices.Equal(got, want) {
		t.Fatalf("lines (project, description) = %v, want %v", got, want)
	}
}

func TestIssuedInvoiceShowsAnAssignedManualLineInItsSection(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))

	inv := c.createAndIssue(pullForm(c, "project", "0"), lineForm("Fee", "1", "10"), assignedLineForm("Domain", "1", "20", hosting))

	wantRows(t, c.get(inv).lineRows(t),
		[]string{"website"},
		[]string{"Working time", "1", "h", "85.00", "85.00", ""},
		[]string{"hosting"},
		[]string{"Domain", "1", "h", "20.00", "20.00", ""},
		[]string{"Fee", "1", "h", "10.00", "10.00", ""},
	)
}

func TestProjectOnAnInvoiceLineCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	_, hosting := c.acmeProjects()
	draft := c.createDraft(pullForm(c, "entry", "0"))
	c.addLine(draft, assignedLineForm("Domain", "1", "20", hosting))

	res := c.postForm("/projects/"+hosting+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project is on an invoice.")
	wantRedirect(t, c.postForm(c.linePath(draft, "Domain")+"/delete", nil), draft)
	wantRedirect(t, c.postForm("/projects/"+hosting+"/delete", nil), c.customerPath("acme")+"/projects")
}
