package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

func quoteForm(customerPath, subject string) url.Values {
	return url.Values{
		"customer":    {strings.TrimPrefix(customerPath, "/customers/")},
		"project":     {""},
		"subject":     {subject},
		"language":    {"en"},
		"pricing":     {"fixed"},
		"valid_until": {"2026-11-25"},
	}
}

var quoteLocation = regexp.MustCompile(`^/quotes/\d+$`)

// createQuote creates a draft quote and returns its path.
func (c *client) createQuote(form url.Values) string {
	c.t.Helper()
	res := c.postForm("/quotes", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !quoteLocation.MatchString(loc) {
		c.t.Fatalf("Location = %q, want /quotes/<id>", loc)
	}
	return loc
}

// acmeQuote creates customer acme and a draft quote for it.
func (c *client) acmeQuote() string {
	c.t.Helper()
	c.createCustomer(customerForm("acme", "85"))
	return c.createQuote(quoteForm(c.customerPath("acme"), "Website relaunch"))
}

func TestNewQuoteIsValidForTheConfiguredDays(t *testing.T) {
	ta := newTestApp(t, devConfig+"[quote]\nvalidity_days = 10\n")
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	res := c.get("/quotes/new")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `name="valid_until" value="2026-11-05"`)
}

func TestQuoteValidityDefaultsToThirtyDays(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	c.get("/quotes/new").wantBody(t, `name="valid_until" value="2026-11-25"`)
}

func TestInvalidQuoteConfigFailsStartup(t *testing.T) {
	for _, toml := range []string{"validity_days = 0", "validity_days = -1"} {
		t.Run(toml, func(t *testing.T) {
			wantStartupError(t, devConfig+"[quote]\n"+toml+"\n", "quote.validity_days")
		})
	}
}

func TestNewQuoteStartsInTheCustomersInvoiceLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	form := customerForm("globex", "90")
	form.Set("invoice_language", "de")
	c.createCustomer(form)
	c.createProject(c.customerPath("globex"), projectForm("portal", ""))

	res := c.get("/quotes/new?customer=" + customerID(c, "globex"))

	res.wantBody(t, `<option value="de" selected>`)
	res.wantBody(t, `>portal</option>`)
	res.wantBody(t, `<option value="fixed" selected>`)
	c.get("/quotes/new?customer="+customerID(c, "acme")).wantBody(t, `<option value="en" selected>`)
}

func TestQuoteDraftKeepsWhatItWasCreatedWith(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	form := quoteForm(cust, "Website relaunch")
	form.Set("project", strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"))
	form.Set("language", "de")
	form.Set("pricing", "effort")
	form.Set("valid_until", "2026-12-01")

	quote := c.createQuote(form)

	res := c.get(quote)
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "acme")
	res.wantBody(t, `selected>website</option>`)
	res.wantBody(t, `name="subject" value="Website relaunch"`)
	res.wantBody(t, `<option value="de" selected>`)
	res.wantBody(t, `<option value="effort" selected>`)
	res.wantBody(t, `name="valid_until" value="2026-12-01"`)
}

func TestQuoteDraftsAreValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))
	c.createProject(c.customerPath("globex"), projectForm("portal", ""))
	acme := c.customerPath("acme")
	for name, tc := range map[string]struct {
		key, value, message string
	}{
		"no subject":       {"subject", " ", "Subject is required."},
		"no customer":      {"customer", "x", "Choose a customer."},
		"other's project":  {"project", strings.TrimPrefix(c.projectPath(c.customerPath("globex"), "portal"), "/projects/"), "Choose a project of this customer."},
		"unknown language": {"language", "fr", "Choose a language."},
		"unknown pricing":  {"pricing", "free", "Choose fixed price or by effort."},
		"invalid date":     {"valid_until", "2026-13-01", "Enter a valid date."},
	} {
		t.Run(name, func(t *testing.T) {
			form := quoteForm(acme, "Website relaunch")
			form.Set(tc.key, tc.value)

			res := c.postForm("/quotes", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
		})
	}
	c.get("/quotes").wantBody(t, "No quotes yet.")
}

func TestQuoteDraftCanBeEdited(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	form := quoteForm(c.customerPath("acme"), "Shop relaunch")
	form.Set("pricing", "effort")
	form.Set("language", "de")
	form.Set("valid_until", "2027-01-15")

	wantRedirect(t, c.postForm(quote, form), quote)

	res := c.get(quote)
	res.wantBody(t, `name="subject" value="Shop relaunch"`)
	res.wantBody(t, `<option value="effort" selected>`)
	res.wantBody(t, `<option value="de" selected>`)
	res.wantBody(t, `name="valid_until" value="2027-01-15"`)

	form.Set("subject", "")
	res = c.postForm(quote, form)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Subject is required.")
}

func quoteLineForm(description, quantity, unitPrice, interval string) url.Values {
	form := lineForm(description, quantity, unitPrice)
	form.Set("interval", interval)
	return form
}

func (c *client) addQuoteLine(quote string, form url.Values) {
	c.t.Helper()
	wantRedirect(c.t, c.postForm(quote+"/lines", form), quote)
}

var quotePartLink = regexp.MustCompile(`href="(/quotes/\d+/(?:lines|sections)/\d+)"[^>]*>([^<]*)<`)

// quotePart returns the path of the line or section on quote whose link reads text.
func (c *client) quotePart(quote, text string) string {
	c.t.Helper()
	for _, m := range quotePartLink.FindAllStringSubmatch(c.get(quote).body, -1) {
		if m[2] == text {
			return m[1]
		}
	}
	c.t.Fatalf("no line or section %q on %s", text, quote)
	return ""
}

func TestQuoteTotalsArePerIntervalAndTaxedLikeInvoices(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	quote := c.acmeQuote()

	c.addQuoteLine(quote, quoteLineForm("Design", "1", "0.13", "once"))
	c.addQuoteLine(quote, quoteLineForm("Build", "1", "0.13", "once"))
	c.addQuoteLine(quote, quoteLineForm("Hosting", "2", "10", "yearly"))

	res := c.get(quote)
	wantInOrder(t, res.text(), "Design", "Once", "0.13", "Build", "Hosting", "Yearly", "20.00")
	res.wantRow(t, "One-off", "0.26 EUR", "0.05 EUR", "0.31 EUR")
	res.wantRow(t, "Per year", "20.00 EUR", "3.80 EUR", "23.80 EUR")
	res.wantNoBody(t, "Per month")
}

func TestQuoteWithoutLinesShowsNoTotals(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()

	res := c.get(quote)

	res.wantBody(t, "No lines yet.")
	res.wantNoBody(t, "One-off")
}

func TestQuoteLinesAreValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	for name, tc := range map[string]struct {
		form    url.Values
		message string
	}{
		"no description": {quoteLineForm("", "1", "85", "once"), "Description is required."},
		"bad quantity":   {quoteLineForm("Design", "1.0005", "85", "once"), "Quantity must be"},
		"bad price":      {quoteLineForm("Design", "1", "85.001", "once"), "Unit price must be"},
		"bad interval":   {quoteLineForm("Design", "1", "85", "weekly"), "Choose an interval."},
	} {
		t.Run(name, func(t *testing.T) {
			res := c.postForm(quote+"/lines", tc.form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
		})
	}
	c.get(quote).wantBody(t, "No lines yet.")
}

func TestQuoteLinesCanBeEditedReorderedAndDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	for _, d := range []string{"Alpha", "Bravo", "Charlie"} {
		c.addQuoteLine(quote, quoteLineForm(d, "1", "10", "once"))
	}

	edit := c.get(c.quotePart(quote, "Bravo"))
	edit.wantBody(t, `name="description" value="Bravo"`)
	edit.wantBody(t, `<option value="once" selected>`)
	form := quoteLineForm("Bravo two", "3.25", "20", "monthly")
	form.Set("detail", "Support")
	wantRedirect(t, c.postForm(c.quotePart(quote, "Bravo"), form), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Charlie")+"/up", nil), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Alpha")+"/down", nil), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Alpha")+"/down", nil), quote)

	res := c.get(quote)
	wantInOrder(t, res.text(), "Charlie", "Bravo two", "Support", "3.25", "20.00", "Monthly", "65.00", "Alpha")
	res.wantRow(t, "One-off", "20.00 EUR", "20.00 EUR")
	res.wantRow(t, "Per month", "65.00 EUR", "65.00 EUR")

	wantRedirect(t, c.postForm(c.quotePart(quote, "Charlie")+"/delete", nil), quote)

	res = c.get(quote)
	res.wantNoBody(t, "Charlie")
	res.wantRow(t, "One-off", "10.00 EUR", "10.00 EUR")
}

func sectionForm(heading, body string) url.Values {
	return url.Values{"heading": {heading}, "body": {body}}
}

func (c *client) addSection(quote string, form url.Values) {
	c.t.Helper()
	wantRedirect(c.t, c.postForm(quote+"/sections", form), quote)
}

func TestQuoteSectionsKeepTheirTypstAsTyped(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	body := "We build *three* pages:\n\n- Home\n- About\n  "

	c.addSection(quote, sectionForm("Scope", body))
	c.addSection(quote, sectionForm("", "Thanks for your interest."))

	res := c.get(c.quotePart(quote, "Scope"))
	res.wantBody(t, `name="heading" value="Scope"`)
	res.wantBody(t, ">"+body+"</textarea>")
	wantInOrder(t, c.get(quote).text(), "Scope", "We build *three* pages:", "Paragraph without heading", "Thanks for your interest.")
}

func TestQuoteSectionsCanBeEditedReorderedAndDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	for _, h := range []string{"Alpha", "Bravo", "Charlie"} {
		c.addSection(quote, sectionForm(h, h+" text"))
	}

	wantRedirect(t, c.postForm(c.quotePart(quote, "Bravo"), sectionForm("Schedule", "In May.")), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Charlie")+"/up", nil), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Alpha")+"/down", nil), quote)
	wantRedirect(t, c.postForm(c.quotePart(quote, "Alpha")+"/down", nil), quote)

	res := c.get(quote)
	wantInOrder(t, res.text(), "Charlie", "Schedule", "In May.", "Alpha")
	res.wantNoBody(t, "Bravo")

	wantRedirect(t, c.postForm(c.quotePart(quote, "Charlie")+"/delete", nil), quote)
	c.get(quote).wantNoBody(t, "Charlie")

	res = c.postForm(quote+"/sections", sectionForm("Empty", " \n"))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Text is required.")
}

func TestQuoteDraftCanBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	c.addQuoteLine(quote, quoteLineForm("Design", "1", "10", "once"))
	c.addSection(quote, sectionForm("Scope", "All of it."))

	wantRedirect(t, c.postForm(quote+"/delete", nil), "/quotes")

	c.get(quote).wantStatus(t, http.StatusNotFound)
	c.postForm(quote+"/lines", quoteLineForm("Late", "1", "10", "once")).wantStatus(t, http.StatusNotFound)
	c.get("/quotes").wantNoBody(t, `href="`+quote+`"`)
	wantRedirect(t, c.postForm(c.customerPath("acme")+"/delete", nil), "/customers")
}

func TestQuotesListNewestFirstAndFilters(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	first := c.acmeQuote()
	c.addQuoteLine(first, quoteLineForm("Design", "2", "100", "once"))
	c.addQuoteLine(first, quoteLineForm("Hosting", "1", "30", "monthly"))
	c.createCustomer(customerForm("globex", "90"))
	second := c.createQuote(quoteForm(c.customerPath("globex"), "Portal"))

	res := c.get("/quotes")
	res.wantRow(t, "Draft", "acme", "Website relaunch", "", "200.00 EUR", "Draft")
	res.wantRow(t, "Draft", "globex", "Portal", "", "0.00 EUR", "Draft")
	wantInOrder(t, res.body, `href="`+second+`"`, `href="`+first+`"`)

	res = c.get("/quotes?customer=" + customerID(c, "globex"))
	res.wantBody(t, `href="`+second+`"`)
	res.wantNoBody(t, `href="`+first+`"`)

	res = c.get("/quotes?status=sent")
	res.wantNoBody(t, `href="`+first+`"`)
	res.wantBody(t, "No quotes match these filters.")
	c.get("/quotes?status=draft").wantBody(t, `href="`+first+`"`)
}

func TestQuotesShowOnTheirCustomerAndProject(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	website := c.projectPath(cust, "website")
	form := quoteForm(cust, "Website relaunch")
	form.Set("project", strings.TrimPrefix(website, "/projects/"))
	forProject := c.createQuote(form)
	newWork := c.createQuote(quoteForm(cust, "Shop"))

	res := c.get(cust).in(t, "Quotes")
	res.wantRow(t, "Draft", "Website relaunch", "", "0.00 EUR", "Draft")
	res.wantBody(t, `href="`+newWork+`"`)

	res = c.get(website).in(t, "Quotes")
	res.wantBody(t, `href="`+forProject+`"`)
	res.wantNoBody(t, `href="`+newWork+`"`)
}

func TestCustomerWithAQuoteCannotBeDeletedOrChangeCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.acmeQuote()
	cust := c.customerPath("acme")

	res := c.postForm(cust+"/delete", nil)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This customer has quotes.")

	form := customerForm("acme", "85")
	form.Set("currency", "CHF")
	res = c.postForm(cust, form)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The currency of a customer with quotes can&#39;t change.")
}

func TestProjectWithAQuoteCannotBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	website := c.projectPath(cust, "website")
	form := quoteForm(cust, "Website relaunch")
	form.Set("project", strings.TrimPrefix(website, "/projects/"))
	c.createQuote(form)

	res := c.postForm(website+"/delete", nil)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This project is on a quote.")
}
