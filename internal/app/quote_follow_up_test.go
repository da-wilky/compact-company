package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

var projectLocation = regexp.MustCompile(`^/projects/\d+$`)

// createProjectFrom creates a project from quote and returns its path.
func (c *client) createProjectFrom(quote string) string {
	c.t.Helper()
	res := c.postForm(quote+"/project", url.Values{})
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !projectLocation.MatchString(loc) {
		c.t.Fatalf("Location = %q, want /projects/<id>", loc)
	}
	return loc
}

// acceptedQuoteWithLines sends and accepts a quote for acme with one-off lines of the given units and unit prices.
func (c *client) acceptedQuoteWithLines(pricing string, lines ...[2]string) string {
	c.t.Helper()
	c.postForm("/settings/business", businessForm()).wantStatus(c.t, http.StatusSeeOther)
	c.createCustomer(customerForm("acme", "85"))
	form := quoteForm(c.customerPath("acme"), "Website relaunch")
	form.Set("pricing", pricing)
	quote := c.createQuote(form)
	for i, l := range lines {
		line := quoteLineForm("Line "+strconv.Itoa(i), "1", l[1], "once")
		line.Set("unit", l[0])
		c.addQuoteLine(quote, line)
	}
	wantRedirect(c.t, c.send(quote), quote)
	wantRedirect(c.t, c.markOutcome(quote, "accept", "2026-11-03"), quote)
	return quote
}

func TestProjectFromAByEffortQuoteBillsAtItsHourlyRate(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acceptedQuoteWithLines("effort", [2]string{"h", "95"}, [2]string{"flat", "300"}, [2]string{"h", "95"})

	project := c.createProjectFrom(quote)

	c.get(c.customerPath("acme")+"/projects").wantRow(t, "", "Website relaunch", "yes", "95.00 EUR")
	c.get(project).in(t, "Quotes").wantRow(t, "A-2026-001", "Website relaunch", "2026-10-26", "583.10 EUR", "Accepted")
	res := c.get(quote)
	res.wantBody(t, `Project <a href="`+project+`">Website relaunch</a>`) // markup is the behaviour: the link
	res.wantNoBody(t, `formaction="`+quote+`/project"`)
	c.get(project).wantBody(t, `From quote <a href="`+quote+`">A-2026-001</a>`) // markup is the behaviour: the link
	c.createProject(c.customerPath("acme"), projectForm("support", ""))
	c.get(c.projectPath(c.customerPath("acme"), "support")).wantNoBody(t, "From quote")
}

func TestProjectFromAQuoteWithoutOneHourlyRateBillsAtTheCustomersRate(t *testing.T) {
	for name, quote := range map[string]func(c *client) string{
		"fixed price":   func(c *client) string { return c.acceptedQuoteWithLines("fixed", [2]string{"h", "95"}) },
		"no hour lines": func(c *client) string { return c.acceptedQuoteWithLines("effort", [2]string{"flat", "300"}) },
		"several hour rates": func(c *client) string {
			return c.acceptedQuoteWithLines("effort", [2]string{"h", "95"}, [2]string{"h", "120"})
		},
	} {
		t.Run(name, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			c := ta.client()

			c.createProjectFrom(quote(c))

			c.get(c.customerPath("acme")+"/projects").wantRow(t, "", "Website relaunch", "yes", "85.00 EUR (customer default)")
		})
	}
}

func TestOnlyAnAcceptedQuoteWithoutAProjectCreatesOne(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	accepted := c.acceptedQuote("effort")
	c.createProjectFrom(accepted)
	sent := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))
	wantRedirect(t, c.send(sent), sent)
	draft := c.createQuote(quoteForm(c.customerPath("acme"), "Support"))

	for _, quote := range []string{accepted, sent, draft, "/quotes/999"} {
		c.postForm(quote+"/project", url.Values{}).wantStatus(t, http.StatusNotFound)
	}
	c.get(sent).wantNoBody(t, `formaction="`+sent+`/project"`)
	c.get(c.customerPath("acme")+"/projects").wantRow(t, "", "Website relaunch", "yes", "85.00 EUR (customer default)")
}

// acceptedQuote sends and accepts a quote for acme with a one-off and a monthly line.
func (c *client) acceptedQuote(pricing string) string {
	c.t.Helper()
	c.postForm("/settings/business", businessForm()).wantStatus(c.t, http.StatusSeeOther)
	c.createCustomer(customerForm("acme", "85"))
	form := quoteForm(c.customerPath("acme"), "Website relaunch")
	form.Set("pricing", pricing)
	quote := c.createQuote(form)
	c.addQuoteLine(quote, quoteLineForm("Design", "2", "500", "once"))
	c.addQuoteLine(quote, quoteLineForm("Hosting", "1", "20", "monthly"))
	wantRedirect(c.t, c.send(quote), quote)
	wantRedirect(c.t, c.markOutcome(quote, "accept", "2026-11-03"), quote)
	return quote
}

// createInvoiceFrom creates an invoice draft from quote and returns its path.
func (c *client) createInvoiceFrom(quote string) string {
	c.t.Helper()
	res := c.postForm(quote+"/invoice", url.Values{})
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !draftLocation.MatchString(loc) {
		c.t.Fatalf("Location = %q, want /invoices/<id>", loc)
	}
	return loc
}

func TestInvoiceFromAFixedPriceQuoteCopiesItsOneOffLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acceptedQuote("fixed")

	draft := c.createInvoiceFrom(quote)

	res := c.get(draft)
	res.wantBody(t, "Draft for acme")
	res.wantRow(t, "Design", "2", "h", "500.00", "1000.00", "")
	res.wantNoBody(t, "Hosting")
	res.wantBody(t, `From quote <a href="`+quote+`">A-2026-001</a>`) // markup is the behaviour: the link
}

func TestInvoiceFromAByEffortQuoteBillsItsProjectWithoutLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("acme", "85"))
	c.createProject(c.customerPath("acme"), projectForm("website", ""))
	c.createProject(c.customerPath("acme"), projectForm("hosting", ""))
	form := quoteForm(c.customerPath("acme"), "Website relaunch")
	form.Set("pricing", "effort")
	form.Set("project", strings.TrimPrefix(c.projectPath(c.customerPath("acme"), "website"), "/projects/"))
	quote := c.createQuote(form)
	c.addQuoteLine(quote, quoteLineForm("Development", "40", "85", "once"))
	wantRedirect(t, c.send(quote), quote)
	wantRedirect(t, c.markOutcome(quote, "accept", "2026-11-03"), quote)

	draft := c.createInvoiceFrom(quote)

	res := c.get(draft)
	res.wantBody(t, "No lines yet.")
	res.wantBody(t, "Projects: website")
}

func TestAQuoteListsTheInvoicesCreatedFromIt(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acceptedQuote("fixed")
	c.get(quote).wantBody(t, `action="`+quote+`/invoice"`)
	issued := c.createInvoiceFrom(quote)
	wantRedirect(t, c.issue(issued), issued)
	draft := c.createInvoiceFrom(quote)

	res := c.get(quote)
	res.wantBody(t, `Invoices: <a href="`+issued+`">2026-001</a>, <a href="`+draft+`">Draft</a>`) // markup is the behaviour: the links
	c.get(issued).wantBody(t, `From quote <a href="`+quote+`">A-2026-001</a>`)
	wantRedirect(t, c.postForm(quote+"/undo", url.Values{}), quote)
	c.get(quote).wantBody(t, `Invoices: <a href="`+issued+`">2026-001</a>`)
}

func TestOnlyAnAcceptedQuoteCreatesAnInvoice(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	sent := c.sentQuote()
	draft := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))
	declined := c.createQuote(quoteForm(c.customerPath("acme"), "Support"))
	wantRedirect(t, c.send(declined), declined)
	wantRedirect(t, c.markOutcome(declined, "decline", "2026-11-03"), declined)

	for _, quote := range []string{sent, draft, declined, "/quotes/999"} {
		c.postForm(quote+"/invoice", url.Values{}).wantStatus(t, http.StatusNotFound)
	}
	c.get(sent).wantNoBody(t, `action="`+sent+`/invoice"`)
	c.get("/invoices").wantBody(t, "No invoices yet.")
}
