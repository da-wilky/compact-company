package app_test

import (
	"html"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

// selectableInvoices lists the invoice paths the list offers to select.
func (r *response) selectableInvoices() []string {
	var paths []string
	for _, m := range selectBox.FindAllStringSubmatch(r.body, -1) {
		paths = append(paths, "/invoices/"+m[1])
	}
	return paths
}

// deleteInvoices posts the selection as the invoice list does: to the list's own query.
func (c *client) deleteInvoices(query string, paths ...string) *response {
	c.t.Helper()
	form := url.Values{}
	for _, p := range paths {
		form.Add("id", strings.TrimPrefix(p, "/invoices/"))
	}
	return c.hxPost("/invoices/delete"+query, form)
}

func TestOnlyDraftsAreSelectable(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	issued := c.issuedWorkshopAndHosting()
	c.cancelLines(issued, "", "Hosting")
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))

	page := c.get("/invoices")

	page.wantBody(t, "Cancellation invoice")
	page.wantBody(t, `href="`+issued+`"`)
	wantPaths(t, page.selectableInvoices(), draft)
}

func TestSelectedDraftsAreDeletedTogetherReleasingTheirEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website, hosting := c.acmeProjects()
	design := c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	server := c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T09:00", "server"))
	chosen := pullForm(c, "entry", "0")
	chosen.Del("all_projects")
	chosen["project"] = []string{website}
	first := c.createDraft(chosen)
	second := c.createDraft(pullForm(c, "entry", "0"))
	kept := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	wantPaths(t, c.get("/entries?day=2026-09-02").selectable())
	query := "?customer=" + customerID(c, "acme") + "&status=draft"
	if got := c.get("/invoices" + query).invoiceDeleteAction(t); got != "/invoices/delete"+query {
		t.Fatalf("selection posts to %q", got)
	}

	res := c.deleteInvoices(query, first, second)

	res.wantStatus(t, http.StatusOK)
	wantPaths(t, res.selectableInvoices(), kept)
	c.get(first).wantStatus(t, http.StatusNotFound)
	c.get(second).wantStatus(t, http.StatusNotFound)
	wantPaths(t, c.get("/entries?day=2026-09-02").selectable(), design)
	wantPaths(t, c.get("/entries?day=2026-09-03").selectable(), server)
}

func TestSelectionWithAnIssuedInvoiceDeletesNothingAndNamesIt(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	issued := c.issuedWorkshopAndHosting()
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))

	res := c.deleteInvoices("", draft, issued)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Nothing was deleted. Only drafts can be deleted, not:", "2026-001 · acme · Issued")
	res.wantBody(t, `value="`+strings.TrimPrefix(draft, "/invoices/")+`" form="invoice-selection" aria-label="Select the draft for acme" checked>`)
	c.get(draft).wantStatus(t, http.StatusOK)
	c.get(issued).wantStatus(t, http.StatusOK)
}

func TestDeletingSelectedDraftsAsksForConfirmationNamingTheCount(t *testing.T) {
	en := newTestApp(t, devConfig).client()
	en.acmeDraft()
	de := newTestApp(t, devConfig+"[defaults]\nlanguage = \"de\"\n").client()
	de.acmeDraft()

	page, seite := en.get("/invoices"), de.get("/invoices")

	wantConfirmation(t, page.selectionQuestion(t, "Delete", 3), "Delete 3 drafts?")
	wantConfirmation(t, page.selectionQuestion(t, "Delete", 1), "Delete 1 draft?")
	wantConfirmation(t, seite.selectionQuestion(t, "Löschen", 3), "3 Entwürfe löschen?")
	wantConfirmation(t, seite.selectionQuestion(t, "Löschen", 1), "1 Entwurf löschen?")
}

var invoiceSelectionForm = regexp.MustCompile(`<form id="invoice-selection"[^>]* action="([^"]*)"`)

func (r *response) invoiceDeleteAction(t *testing.T) string {
	t.Helper()
	m := invoiceSelectionForm.FindStringSubmatch(r.body)
	if m == nil {
		t.Fatalf("no selection form:\n%s", r.body)
	}
	return html.UnescapeString(m[1])
}
