package app_test

import (
	"html"
	"regexp"
	"testing"
)

var confirmAttr = regexp.MustCompile(`data-confirm="([^"]*)"`)

// confirmation is the question the page asks before submitting the form that posts to action.
func (r *response) confirmation(t *testing.T, action string) string {
	t.Helper()
	form := regexp.MustCompile(`<form[^>]*action="` + regexp.QuoteMeta(action) + `"[^>]*>`).FindString(r.body)
	if form == "" {
		t.Fatalf("no form posting to %s:\n%s", action, r.body)
	}
	m := confirmAttr.FindStringSubmatch(form)
	if m == nil {
		t.Fatalf("form posting to %s asks no confirmation: %s", action, form)
	}
	return html.UnescapeString(m[1])
}

func wantConfirmation(t *testing.T, got, want string) {
	t.Helper()
	if got != want {
		t.Fatalf("confirmation = %q, want %q", got, want)
	}
}

func TestDeletingACustomerAsksForConfirmation(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	wantConfirmation(t, c.get("/customers/1").confirmation(t, "/customers/1/delete"), "Delete customer K-001 acme?")
}

func TestDeletingAProjectAsksForConfirmation(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	wantConfirmation(t, c.get("/projects/"+project).confirmation(t, "/projects/"+project+"/delete"), "Delete project website?")
}

func TestDeletingAnEntryAsksForConfirmationNamingItsStartAndDescription(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	entry := c.createEntry(entryForm(project, "2026-10-26T08:00", "2026-10-26T09:00", "kickoff"))

	wantConfirmation(t, c.get(entry).confirmation(t, entry+"/delete"), "Delete the time entry of 2026-10-26 08:00 “kickoff”?")
}

func TestDeletingADraftOrItsLineAsksForConfirmation(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	c.addLine(draft, lineForm("Workshop", "1", "500"))
	line := c.linePath(draft, "Workshop")

	page := c.get(draft)

	wantConfirmation(t, page.confirmation(t, draft+"/delete"), "Delete the draft for K-001 acme?")
	wantConfirmation(t, page.confirmation(t, line+"/delete"), "Delete the line “Workshop”?")
}

func TestDeletingAnEntryWithoutDescriptionAsksInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig+"[defaults]\nlanguage = \"de\"\n")
	c := ta.client()
	entry := c.createEntry(entryForm("", "2026-10-26T08:00", "2026-10-26T09:00", ""))

	wantConfirmation(t, c.get(entry).confirmation(t, entry+"/delete"), "Zeiteintrag vom 26.10.2026 08:00 löschen?")
}

func TestUndoingPaidAsksForConfirmation(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuableDraft()
	wantRedirect(t, c.issue(invoice), invoice)
	wantRedirect(t, c.markPaid(invoice, "2026-11-03"), invoice)

	wantConfirmation(t, c.get(invoice).confirmation(t, invoice+"/unpaid"), "Undo the payment of invoice 2026-001?")
}
