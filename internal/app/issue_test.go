package app_test

import (
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"net/url"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// issuableDraft sets up business data and a September draft for acme with one line.
func (c *client) issuableDraft() string {
	c.t.Helper()
	c.postForm("/settings/business", businessForm()).wantStatus(c.t, http.StatusSeeOther)
	draft := c.acmeDraft()
	c.addLine(draft, lineForm("Workshop", "1.5", "85"))
	return draft
}

func (c *client) issue(draft string) *response {
	c.t.Helper()
	return c.postForm(draft+"/issue", url.Values{})
}

// boomConfig renders through a fixture template that fails only for a line described "boom".
func boomConfig(t *testing.T) string {
	t.Helper()
	dir, err := filepath.Abs("testdata/boom")
	if err != nil {
		t.Fatal(err)
	}
	return devConfig + "[invoice]\ntemplate_dir = " + strconv.Quote(dir) + "\n" + businessConfig
}

func TestIssuingNumbersInvoicesGaplesslyPerIssueYear(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	first := c.issuableDraft()
	second := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(second, lineForm("Hosting", "1", "10"))
	third := c.createDraft(draftForm(c.customerPath("acme"), "2026-12-01", "2026-12-31"))
	c.addLine(third, lineForm("Hosting", "1", "10"))

	wantRedirect(t, c.issue(first), first)
	wantRedirect(t, c.issue(second), second)
	ta.clock.Advance(66*24*time.Hour + 14*time.Hour + 30*time.Minute) // 2027-01-01 00:30 in Berlin, still 2026 in UTC
	wantRedirect(t, c.issue(third), third)

	c.get(first).wantBody(t, "Invoice 2026-001")
	c.get(second).wantBody(t, "Invoice 2026-002")
	res := c.get(third)
	res.wantBody(t, "Invoice 2027-001")
	res.wantBody(t, "Issue date 2027-01-01")
	wantInOrder(t, c.get("/invoices").body, "2027-001", "2026-002", "2026-001")
}

func TestIssuingPassesTheNumberAndDatesToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	draft := c.acmeDraft()
	ta.clock.Advance(14*time.Hour + 30*time.Minute) // 00:30 on the 27th in Berlin

	res := c.issue(draft)

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The invoice template failed to render")
	input := res.templateInput(t).(map[string]any)
	for key, want := range map[string]any{"number": "2026-001", "draft": false, "issue_date": "2026-10-27", "due_date": "2026-11-10"} {
		if input[key] != want {
			t.Errorf("%s = %v, want %v", key, input[key], want)
		}
	}
}

func TestInvoiceNumberFormatIsConfigurable(t *testing.T) {
	ta := newTestApp(t, strings.Replace(dumpConfig(t), "[invoice]\n", "[invoice]\nnumber_format = \"RE{seq:04}_{year}\"\n", 1))
	c := ta.client()
	draft := c.acmeDraft()

	number := c.issue(draft).templateInput(t).(map[string]any)["number"]

	if number != "RE0001_2026" {
		t.Fatalf("number = %v, want RE0001_2026", number)
	}
}

func TestInvoiceNumberFormatMustBeASafeFileName(t *testing.T) {
	for _, format := range []string{"{year}/{seq:03}", "..{year}-{seq}", "{year}-{seq}..", "{seq:03}", "{year}", "{year}-{seq}-{seq}"} {
		t.Run(format, func(t *testing.T) {
			wantStartupError(t, devConfig+"[invoice]\nnumber_format = "+strconv.Quote(format)+"\n", "invoice.number_format")
		})
	}
}

func TestFailingRenderIssuesNothingAndLeavesTheNumberUnused(t *testing.T) {
	ta := newTestApp(t, boomConfig(t))
	c := ta.client()
	broken := c.acmeDraft()
	c.addLine(broken, lineForm("boom", "1", "10"))
	good := c.createDraft(draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31"))
	c.addLine(good, lineForm("Hosting", "1", "10"))

	res := c.issue(broken)

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The invoice template failed to render")
	res.wantBody(t, "boom line")
	c.get(broken).wantBody(t, "Draft for acme")
	c.get(broken+"/pdf").wantStatus(t, http.StatusNotFound)
	wantRedirect(t, c.issue(good), good)
	c.get(good).wantBody(t, "Invoice 2026-001")
}

func TestIssuingIsRefusedWhileBusinessDataIsMissing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()

	res := c.issue(draft)

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Invoices can&#39;t be previewed or issued until these are set: Name, Address, Tax number")
	c.get(draft).wantBody(t, "Draft for acme")
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	wantRedirect(t, c.issue(draft), draft)
	c.get(draft).wantBody(t, "Invoice 2026-001")
}

func TestIssuedInvoicesAndTheirLinesCantBeChanged(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.issuableDraft()
	line := c.linePath(draft, "Workshop")
	wantRedirect(t, c.issue(draft), draft)

	header := url.Values{"from": {"2026-09-01"}, "to": {"2026-09-30"}, "service_from": {"2026-09-01"}, "service_to": {"2026-09-30"}, "payment_days": {"30"}}
	for path, form := range map[string]url.Values{
		draft:              header,
		draft + "/delete":  {},
		draft + "/lines":   lineForm("Extra", "1", "10"),
		draft + "/refresh": {},
		draft + "/issue":   {},
		line:               lineForm("Changed", "2", "10"),
		line + "/up":       {},
		line + "/down":     {},
		line + "/delete":   {},
	} {
		c.postForm(path, form).wantStatus(t, http.StatusNotFound)
	}
	for _, path := range []string{draft + "/lines/new", line, draft + "/refresh", draft + "/issue", draft + "/preview"} {
		c.get(path).wantStatus(t, http.StatusNotFound)
	}
	res := c.get(draft)
	res.wantBody(t, "Invoice 2026-001")
	wantInOrder(t, res.body, "Workshop", "1.5", "85.00", "127.50", "127.50", "24.23", "151.73")
	res.wantNoBody(t, "Changed")
	res.wantNoBody(t, "Extra")
}

func TestDownloadServesThePDFStoredAtIssuing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.issuableDraft()
	c.get(draft+"/pdf").wantStatus(t, http.StatusNotFound)
	wantRedirect(t, c.issue(draft), draft)

	pdf := c.get(draft + "/pdf")

	pdf.wantStatus(t, http.StatusOK)
	if ct := pdf.Header.Get("Content-Type"); ct != "application/pdf" {
		t.Fatalf("Content-Type = %q", ct)
	}
	if cd := pdf.Header.Get("Content-Disposition"); cd != "attachment; filename=2026-001.pdf" {
		t.Fatalf("Content-Disposition = %q", cd)
	}
	if !strings.HasPrefix(pdf.body, "%PDF-") {
		t.Fatalf("body is not a PDF: %.40q", pdf.body)
	}
	sum := sha256.Sum256([]byte(pdf.body))
	c.get(draft).wantBody(t, hex.EncodeToString(sum[:]))
	ta.restart()
	if again := c.get(draft + "/pdf"); again.body != pdf.body {
		t.Fatal("second download differs from the first")
	}
}

func TestLaterBusinessAndCustomerChangesLeaveIssuedInvoicesAlone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.issuableDraft()
	wantRedirect(t, c.issue(draft), draft)
	pdf := c.get(draft + "/pdf").body

	business := businessForm()
	business.Set("name", "Someone Else")
	business.Set("tax_mode", "small_business")
	c.postForm("/settings/business", business).wantStatus(t, http.StatusSeeOther)
	c.postForm(c.customerPath("acme"), customerForm("Globex AG", "99")).wantStatus(t, http.StatusSeeOther)

	res := c.get(draft)
	res.wantBody(t, "K-001 acme ·")
	wantInOrder(t, res.body, "127.50", "24.23", "151.73")
	list := c.get("/invoices")
	wantInOrder(t, list.body, "2026-001", "acme", "151.73 EUR")
	if c.get(draft+"/pdf").body != pdf {
		t.Fatal("stored PDF changed")
	}
}
