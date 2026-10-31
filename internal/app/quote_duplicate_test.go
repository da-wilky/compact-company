package app_test

import (
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

// duplicateQuote duplicates quote for the customer at customerPath and returns the new draft's path.
func (c *client) duplicateQuote(quote, customerPath string) string {
	c.t.Helper()
	res := c.postForm(quote+"/duplicate", url.Values{"customer": {strings.TrimPrefix(customerPath, "/customers/")}})
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !quoteLocation.MatchString(loc) || loc == quote {
		c.t.Fatalf("Location = %q, want a new /quotes/<id>", loc)
	}
	return loc
}

// fullQuote creates customer acme with project website and a German by-effort draft for it with lines and sections.
func (c *client) fullQuote() string {
	c.t.Helper()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	form := quoteForm(cust, "Website relaunch")
	form.Set("project", strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"))
	form.Set("language", "de")
	form.Set("pricing", "effort")
	form.Set("valid_until", "2027-01-15")
	quote := c.createQuote(form)
	design := quoteLineForm("Design", "2", "100.50", "once")
	design.Set("detail", "Two drafts")
	design.Set("unit", "days")
	c.addQuoteLine(quote, design)
	c.addQuoteLine(quote, quoteLineForm("Hosting", "1", "30", "monthly"))
	c.addSection(quote, sectionForm("Scope", "We build *three* pages."))
	c.addSection(quote, sectionForm("", "Thanks for your interest."))
	return quote
}

func TestDuplicateCopiesAQuoteIntoADraftForAnotherCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.fullQuote()
	globex := customerForm("globex", "90")
	globex.Set("invoice_language", "en")
	c.createCustomer(globex)
	ta.clock.Advance(24 * time.Hour)

	dup := c.duplicateQuote(quote, c.customerPath("globex"))

	res := c.get(dup)
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "globex")
	res.wantBody(t, `name="subject" value="Website relaunch"`)
	res.wantBody(t, `<option value="effort" selected>`)
	res.wantBody(t, `<option value="en" selected>`)
	res.wantBody(t, `name="valid_until" value="2026-11-26"`)
	c.get(c.projectPath(c.customerPath("acme"), "website")).in(t, "Quotes").wantNoBody(t, `href="`+dup+`"`)
	wantInOrder(t, res.text(), "Design", "Two drafts", "2", "days", "100.50", "Once", "201.00",
		"Hosting", "Monthly", "30.00", "Scope", "We build *three* pages.", "Paragraph without heading", "Thanks for your interest.")
	res.wantRow(t, "One-off", "201.00 EUR", "201.00 EUR")

	orig := c.get(quote)
	orig.wantBody(t, `<option value="de" selected>`)
	orig.wantBody(t, `selected>website</option>`)
}

func TestDuplicateValidityCountsFromTodayInTheNewCustomersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	form := customerForm("kiribati", "85")
	form.Set("timezone", "Pacific/Kiritimati")
	c.createCustomer(form)
	ta.clock.Advance(2 * time.Hour) // noon on the 26th in Berlin, 01:00 on the 27th in Kiritimati

	c.get(c.duplicateQuote(quote, c.customerPath("kiribati"))).wantBody(t, `name="valid_until" value="2026-11-26"`)
	c.get(c.duplicateQuote(quote, c.customerPath("acme"))).wantBody(t, `name="valid_until" value="2026-11-25"`)
}

func TestDuplicateForTheSameCustomerKeepsTheProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.fullQuote()

	dup := c.duplicateQuote(quote, c.customerPath("acme"))

	res := c.get(dup)
	res.wantBody(t, `selected>website</option>`)
	res.wantBody(t, `<option value="en" selected>`)
	res.wantBody(t, `name="valid_until" value="2026-11-25"`)
	wantInOrder(t, res.text(), "Design", "Hosting", "Scope")
}

func TestDuplicateRescalesPricesToTheNewCustomersCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.fullQuote()
	yen := customerForm("nippon", "9000")
	yen.Set("currency", "JPY")
	c.createCustomer(yen)

	res := c.get(c.duplicateQuote(quote, c.customerPath("nippon")))

	wantInOrder(t, res.text(), "Design", "101", "202", "Hosting", "30")
	res.wantRow(t, "One-off", "202 JPY", "202 JPY")
}

func TestASentQuoteCanBeDuplicated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	quote := c.fullQuote()
	wantRedirect(t, c.send(quote), quote)

	c.get(quote+"/duplicate").wantStatus(t, http.StatusOK)
	res := c.get(c.duplicateQuote(quote, c.customerPath("acme")))

	res.wantBody(t, `name="subject" value="Website relaunch"`)
	wantInOrder(t, res.text(), "Design", "Hosting", "Scope")
	c.get("/quotes?status=sent").wantBody(t, "A-2026-001")
}

func TestDuplicateFormOffersEveryCustomerStartingWithTheQuotes(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("globex", "90"))
	quote := c.acmeQuote()

	res := c.get(quote + "/duplicate")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, ">globex</option>")
	res.wantBody(t, " selected>acme</option>")
	res.wantBody(t, `href="`+quote+`"`)
	c.get(quote).wantBody(t, `href="`+quote+`/duplicate"`)
}

func TestDuplicateRefusesUnknownQuotesAndCustomers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()

	c.get("/quotes/999/duplicate").wantStatus(t, http.StatusNotFound)
	c.postForm("/quotes/999/duplicate", url.Values{"customer": {customerID(c, "acme")}}).wantStatus(t, http.StatusNotFound)
	res := c.postForm(quote+"/duplicate", url.Values{"customer": {"999"}})
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Choose a customer.")
	c.get("/quotes").wantRow(t, "Draft", "acme", "Website relaunch", "", "0.00 EUR", "Draft")
}
