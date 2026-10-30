package app_test

import (
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"testing"
	"time"
)

func (c *client) send(quote string) *response {
	c.t.Helper()
	return c.postForm(quote+"/send", url.Values{})
}

// sendableQuote sets up business data and a draft quote for acme with one line.
func (c *client) sendableQuote() string {
	c.t.Helper()
	c.postForm("/settings/business", businessForm()).wantStatus(c.t, http.StatusSeeOther)
	quote := c.acmeQuote()
	c.addQuoteLine(quote, quoteLineForm("Design", "1", "1000", "once"))
	return quote
}

func TestSendingNumbersQuotesGaplesslyPerSendYear(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	first := c.sendableQuote()
	second := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))
	third := c.createQuote(quoteForm(c.customerPath("acme"), "Support"))

	wantRedirect(t, c.send(first), first)
	wantRedirect(t, c.send(second), second)
	ta.clock.Advance(66*24*time.Hour + 14*time.Hour + 30*time.Minute) // 2027-01-01 00:30 in Berlin, still 2026 in UTC
	wantRedirect(t, c.send(third), third)

	c.get(first).wantBody(t, "Quote A-2026-001")
	c.get(second).wantBody(t, "Quote A-2026-002")
	res := c.get(third)
	res.wantBody(t, "Quote A-2027-001")
	res.wantBody(t, "Sent 2027-01-01")
	res = c.get("/quotes")
	res.wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Sent")
	wantInOrder(t, res.body, "A-2027-001", "A-2026-002", "A-2026-001")
}

func TestSendingPassesTheNumberAndSendDateToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	quote := c.acmeQuote()
	ta.clock.Advance(14*time.Hour + 30*time.Minute) // 00:30 on the 27th in Berlin

	res := c.send(quote)

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The template failed to render")
	input := res.templateInput(t).(map[string]any)
	for key, want := range map[string]any{"kind": "quote", "number": "A-2026-001", "draft": false, "issue_date": "2026-10-27"} {
		if input[key] != want {
			t.Errorf("%s = %v, want %v", key, input[key], want)
		}
	}
}

func TestQuoteNumberFormatIsConfigurable(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t)+"[quote]\nnumber_format = \"ANG{seq:04}_{year}\"\n")
	c := ta.client()
	quote := c.acmeQuote()

	number := c.send(quote).templateInput(t).(map[string]any)["number"]

	if number != "ANG0001_2026" {
		t.Fatalf("number = %v, want ANG0001_2026", number)
	}
}

func TestQuoteNumberFormatMustBeASafeFileName(t *testing.T) {
	for _, format := range []string{"{year}/{seq:03}", "..{year}-{seq}", "{year}-{seq}..", "{seq:03}", "{year}", "{year}-{seq}-{seq}"} {
		t.Run(format, func(t *testing.T) {
			wantStartupError(t, devConfig+"[quote]\nnumber_format = "+strconv.Quote(format)+"\n", "quote.number_format")
		})
	}
}

func TestFailingRenderSendsNothingAndLeavesTheQuoteNumberUnused(t *testing.T) {
	ta := newTestApp(t, boomConfig(t))
	c := ta.client()
	broken := c.acmeQuote()
	c.addQuoteLine(broken, quoteLineForm("boom", "1", "10", "once"))
	good := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))

	res := c.send(broken)

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The template failed to render")
	res.wantBody(t, "boom line")
	c.get(broken).wantBody(t, "Draft for K-001 acme")
	c.get(broken+"/pdf").wantStatus(t, http.StatusNotFound)
	wantRedirect(t, c.send(good), good)
	c.get(good).wantBody(t, "Quote A-2026-001")
}

func TestSendingIsRefusedWhileBusinessDataIsMissing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()

	res := c.send(quote)

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Invoices and quotes can&#39;t be previewed, issued or sent until these are set: Name, Address, Tax number")
	c.get(quote).wantBody(t, "Draft for K-001 acme")
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	wantRedirect(t, c.send(quote), quote)
	c.get(quote).wantBody(t, "Quote A-2026-001")
}

func TestSendingIsRefusedWhileASectionFailsToCompile(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sendableQuote()
	c.addSection(quote, sectionForm("Price", "It costs 5$ only."))

	confirm := c.get(quote + "/send")
	res := c.send(quote)

	confirm.wantBody(t, "A section doesn&#39;t compile. Fix it on the quote before sending.")
	confirm.wantNoBody(t, `action="`+quote+`/send"`)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "A section doesn&#39;t compile. Fix it on the quote before sending.")
	c.get(quote).wantBody(t, "Draft for K-001 acme")
	c.postForm(c.quotePart(quote, "Price"), sectionForm("Price", "It costs 5\\$ only.")).wantStatus(t, http.StatusSeeOther)
	c.get(quote+"/send").wantBody(t, `action="`+quote+`/send"`)
	wantRedirect(t, c.send(quote), quote)
}

func TestSentQuotesTheirLinesAndSectionsCantBeChanged(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sendableQuote()
	c.addSection(quote, sectionForm("Scope", "Three pages."))
	line := c.quotePart(quote, "Design")
	section := c.quotePart(quote, "Scope")
	wantRedirect(t, c.send(quote), quote)

	header := quoteForm(c.customerPath("acme"), "Changed")
	for path, form := range map[string]url.Values{
		quote:               header,
		quote + "/delete":   {},
		quote + "/send":     {},
		quote + "/lines":    quoteLineForm("Extra", "1", "10", "once"),
		line:                quoteLineForm("Changed", "2", "10", "once"),
		line + "/up":        {},
		line + "/down":      {},
		line + "/delete":    {},
		quote + "/sections": sectionForm("Extra", "More."),
		section:             sectionForm("Changed", "Changed."),
		section + "/up":     {},
		section + "/down":   {},
		section + "/delete": {},
	} {
		c.postForm(path, form).wantStatus(t, http.StatusNotFound)
	}
	for _, path := range []string{quote + "/lines/new", line, quote + "/sections/new", section, quote + "/send", quote + "/preview"} {
		c.get(path).wantStatus(t, http.StatusNotFound)
	}
	res := c.get(quote)
	res.wantBody(t, "Quote A-2026-001")
	wantInOrder(t, res.text(), "Website relaunch", "Design", "1000.00", "Once", "1000.00", "One-off", "1000.00", "190.00", "1190.00", "Scope", "Three pages.")
	res.wantNoBody(t, "Changed")
	res.wantNoBody(t, "Extra")
}

func TestSentQuoteShowsWhatWasSentAndOffersDownloadAndDuplicate(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sendableQuote()
	wantRedirect(t, c.send(quote), quote)
	form := customerForm("acme renamed", "85")
	c.postForm(c.customerPath("acme"), form).wantStatus(t, http.StatusSeeOther)
	vat := businessForm()
	vat.Set("vat_rate", "7")
	c.postForm("/settings/business", vat).wantStatus(t, http.StatusSeeOther)

	res := c.get(quote)

	res.wantBody(t, "Quote A-2026-001 for K-001 acme")
	res.wantBody(t, "Sent 2026-10-26")
	res.wantBody(t, `href="`+quote+`/pdf"`)
	res.wantBody(t, `href="`+quote+`/duplicate"`)
	res.wantNoBody(t, `href="`+quote+`/send"`)
	res.wantRow(t, "One-off", "1000.00 EUR", "190.00 EUR", "1190.00 EUR")
	c.get("/quotes").wantRow(t, "A-2026-001", "acme renamed", "Website relaunch", "2026-10-26", "1190.00 EUR", "Sent")
	dup := c.get(quote + "/duplicate")
	dup.wantStatus(t, http.StatusOK)
	dup.wantBody(t, `href="`+quote+`"`)
}

func TestQuoteDownloadServesThePDFStoredAtSending(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sendableQuote()
	c.get(quote+"/pdf").wantStatus(t, http.StatusNotFound)
	wantRedirect(t, c.send(quote), quote)

	pdf := c.get(quote + "/pdf")

	pdf.wantStatus(t, http.StatusOK)
	if ct := pdf.Header.Get("Content-Type"); ct != "application/pdf" {
		t.Fatalf("Content-Type = %q", ct)
	}
	if cd := pdf.Header.Get("Content-Disposition"); cd != "attachment; filename=A-2026-001.pdf" {
		t.Fatalf("Content-Disposition = %q", cd)
	}
	if !strings.HasPrefix(pdf.body, "%PDF-") {
		t.Fatalf("body is not a PDF: %.40q", pdf.body)
	}
	sum := sha256.Sum256([]byte(pdf.body))
	c.get(quote).wantBody(t, hex.EncodeToString(sum[:]))
	ta.restart()
	if again := c.get(quote + "/pdf"); again.body != pdf.body {
		t.Fatal("second download differs from the first")
	}
}
