package app_test

import (
	"net/http"
	"net/url"
	"path/filepath"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

var cancelCheckbox = regexp.MustCompile(`name="line" value="(\d+)">\s*([^<]*)<`)

// cancelLines cancels the lines described by descriptions on issued invoice inv
// and returns the cancellation invoice's path.
func (c *client) cancelLines(inv, reason string, descriptions ...string) string {
	c.t.Helper()
	res := c.postForm(inv+"/cancel", c.cancelForm(inv, reason, descriptions...))
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !draftLocation.MatchString(loc) || loc == inv {
		c.t.Fatalf("Location = %q, want the cancellation invoice", loc)
	}
	return loc
}

func (c *client) cancelForm(inv, reason string, descriptions ...string) url.Values {
	c.t.Helper()
	ids := map[string]string{}
	for _, m := range cancelCheckbox.FindAllStringSubmatch(c.get(inv+"/cancel").body, -1) {
		ids[m[2]] = m[1]
	}
	form := url.Values{"reason": {reason}}
	for _, d := range descriptions {
		id, ok := ids[d]
		if !ok {
			c.t.Fatalf("no cancellable line %q on %s", d, inv)
		}
		form.Add("line", id)
	}
	return form
}

// issuedWorkshopAndHosting issues invoice 2026-001 for acme: Workshop 1.5 h × 85 and Hosting 2 h × 10.
func (c *client) issuedWorkshopAndHosting() string {
	c.t.Helper()
	inv := c.issuableDraft()
	c.addLine(inv, lineForm("Hosting", "2", "10"))
	wantRedirect(c.t, c.issue(inv), inv)
	return inv
}

func TestCancellingLinesIssuesACancellationInvoiceFromTheSameSequence(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()

	cancellation := c.cancelLines(inv, "wrong rate", "Workshop")

	res := c.get(cancellation)
	res.wantBody(t, "Cancellation invoice 2026-002")
	res.wantBody(t, "wrong rate")
	res.wantBody(t, `href="`+inv+`">2026-001<`)
	wantInOrder(t, res.body, "Workshop", "-1.5", "85.00", "-127.50", "-127.50", "-24.23", "-151.73")
	res.wantNoBody(t, "Hosting")
	res.wantNoBody(t, `href="`+cancellation+`/cancel"`)
	c.get(cancellation+"/pdf").wantStatus(t, http.StatusOK)
	next := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(next, lineForm("Extra", "1", "10"))
	wantRedirect(t, c.issue(next), next)
	c.get(next).wantBody(t, "Invoice 2026-003")
}

func TestInvoiceIsPartiallyCancelledWithTheAmountStillDueThenCancelled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()

	first := c.cancelLines(inv, "", "Hosting")

	res := c.get(inv)
	res.wantBody(t, "Partially cancelled")
	res.wantBody(t, `href="`+first+`">Cancelled by 2026-002<`)
	wantInOrder(t, res.body, "147.50", "28.03", "175.53", "Amount still due", "151.73")
	res.wantBody(t, `href="`+inv+`/cancel"`)

	c.cancelLines(inv, "", "Workshop")

	res = c.get(inv)
	res.wantBody(t, "K-001 acme · Cancelled")
	wantInOrder(t, res.body, "Amount still due", "0.00")
	res.wantNoBody(t, `href="`+inv+`/cancel"`)
}

func TestALineIsCancelledAtMostOnce(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()
	twice := c.cancelForm(inv, "", "Hosting")
	c.cancelLines(inv, "", "Hosting")

	res := c.postForm(inv+"/cancel", twice)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Tick at least one line of this invoice that isn&#39;t cancelled yet.")
	c.postForm(inv+"/cancel", url.Values{}).wantStatus(t, http.StatusUnprocessableEntity)
	page := c.get(inv + "/cancel")
	page.wantBody(t, "Hosting · Cancelled by 2026-002")
	c.cancelForm(inv, "", "Workshop")
	c.get("/invoices").wantNoBody(t, "2026-004")
}

func TestCancellingALineReleasesItsEntriesForANewDraft(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	website, hosting := c.acmeProjects()
	c.createEntry(entryForm(website, "2026-09-02T08:00", "2026-09-02T09:00", "design"))
	c.createEntry(entryForm(hosting, "2026-09-03T08:00", "2026-09-03T08:30", "server"))
	inv := c.createDraft(pullForm(c, "project", "0"))
	wantRedirect(t, c.issue(inv), inv)

	c.cancelLines(inv, "", "website")

	again := c.createDraft(pullForm(c, "project", "0"))
	res := c.get(again)
	wantInOrder(t, res.body, ">website<", "1", "85.00", "85.00")
	res.wantNoBody(t, ">hosting</a>")
}

func TestInvoiceListShowsCancellationStatusesAndFiltersCancelled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	partly := c.issuedWorkshopAndHosting()
	whole := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(whole, lineForm("Extra", "1", "10"))
	wantRedirect(t, c.issue(whole), whole)
	open := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(open, lineForm("Open", "1", "10"))
	wantRedirect(t, c.issue(open), open)
	c.cancelLines(partly, "", "Hosting")
	c.cancelLines(whole, "", "Extra")

	list := c.get("/invoices").body
	wantInOrder(t, list, "2026-005", "Cancellation invoice", "2026-004", "Cancellation invoice", "2026-003", "Issued",
		"2026-002", "Cancelled", "2026-001", "Partially cancelled")

	cancelled := c.get("/invoices?status=cancelled")
	wantInOrder(t, cancelled.body, `href="`+whole+`"`, `href="`+partly+`"`)
	cancelled.wantNoBody(t, `href="`+open+`"`)
	cancelled.wantNoBody(t, "2026-004")
	issued := c.get("/invoices?status=issued")
	issued.wantBody(t, `href="`+open+`"`)
	issued.wantNoBody(t, `href="`+partly+`"`)
}

// dumpCancellationConfig renders invoices but fails for cancellations with the JSON they received.
func dumpCancellationConfig(t *testing.T) string {
	t.Helper()
	dir, err := filepath.Abs("testdata/dump-cancellation")
	if err != nil {
		t.Fatal(err)
	}
	return devConfig + "[invoice]\ntemplate_dir = " + strconv.Quote(dir) + "\n"
}

func TestCancellationPassesTheContractWithTheOriginalTaxToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpCancellationConfig(t))
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()
	business := businessForm()
	business.Set("tax_mode", "small_business")
	c.postForm("/settings/business", business).wantStatus(t, http.StatusSeeOther)

	res := c.postForm(inv+"/cancel", c.cancelForm(inv, "wrong rate", "Workshop"))

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The invoice template failed to render")
	input := res.templateInput(t).(map[string]any)
	want := decodeJSON(t, `{
		"kind": "cancellation", "draft": false, "number": "2026-002", "issue_date": "2026-10-26",
		"cancels": {"number": "2026-001", "issue_date": "2026-10-26"}, "reason": "wrong rate",
		"tax": {"small_business": false, "vat_rate": 19},
		"lines": [{"description": "Workshop", "detail": "", "quantity": "-1.5", "unit": "h", "unit_price": "85.00", "total": "-127.50", "entries": []}],
		"totals": {"net": "-127.50", "vat": "-24.23", "gross": "-151.73"}
	}`).(map[string]any)
	for key, w := range want {
		if !reflect.DeepEqual(input[key], w) {
			t.Errorf("%s = %v, want %v", key, input[key], w)
		}
	}
	page := c.get(inv)
	page.wantBody(t, "K-001 acme · Issued")
	c.cancelForm(inv, "", "Workshop")
	c.get("/invoices").wantNoBody(t, "2026-002")
}

func TestCancellingIsRefusedWhileBusinessDataIsMissing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()
	business := businessForm()
	business.Set("name", "")
	c.postForm("/settings/business", business).wantStatus(t, http.StatusSeeOther)

	res := c.postForm(inv+"/cancel", c.cancelForm(inv, "", "Workshop"))

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Invoices can&#39;t be previewed or issued until these are set: Name")
	c.get(inv).wantBody(t, "K-001 acme · Issued")
}

func TestDefaultTemplateRendersACancellationInvoice(t *testing.T) {
	for _, language := range []string{"en", "de"} {
		t.Run(language, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			c := ta.client()
			c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
			form := customerForm("acme", "85")
			form.Set("invoice_language", language)
			c.createCustomer(form)
			inv := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))
			c.addLine(inv, lineForm("Workshop", "1.5", "85"))
			wantRedirect(t, c.issue(inv), inv)

			cancellation := c.cancelLines(inv, "wrong rate", "Workshop")

			pdf := c.get(cancellation + "/pdf")
			pdf.wantStatus(t, http.StatusOK)
			if !strings.HasPrefix(pdf.body, "%PDF-") {
				t.Fatalf("body is not a PDF: %.40q", pdf.body)
			}
		})
	}
}

func TestCancellationInvoicesCantBeCancelledNorDraftsCancelled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuedWorkshopAndHosting()
	cancellation := c.cancelLines(inv, "", "Hosting")
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(draft, lineForm("Extra", "1", "10"))

	for _, path := range []string{cancellation, draft} {
		c.get(path+"/cancel").wantStatus(t, http.StatusNotFound)
		c.postForm(path+"/cancel", url.Values{"line": {"1"}}).wantStatus(t, http.StatusNotFound)
	}
}
