package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"slices"
	"strings"
	"testing"
)

func draftForm(customerPath, from, to string) url.Values {
	return url.Values{
		"customer":         {strings.TrimPrefix(customerPath, "/customers/")},
		"from":             {from},
		"to":               {to},
		"grouping":         {"project"},
		"rounding_minutes": {"15"},
		"payment_days":     {"14"},
	}
}

var draftLocation = regexp.MustCompile(`^/invoices/\d+$`)

// createDraft creates a draft and returns its path.
func (c *client) createDraft(form url.Values) string {
	c.t.Helper()
	res := c.postForm("/invoices", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	loc := res.Header.Get("Location")
	if !draftLocation.MatchString(loc) {
		c.t.Fatalf("Location = %q, want /invoices/<id>", loc)
	}
	return loc
}

// acmeDraft creates customer acme and a September draft for it.
func (c *client) acmeDraft() string {
	c.t.Helper()
	c.createCustomer(customerForm("acme", "85"))
	return c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))
}

func TestNewDraftFormDefaultsFromConfigAndLastMonth(t *testing.T) {
	ta := newTestApp(t, devConfig+"[invoice]\npayment_days = 30\nrounding_minutes = 15\n")
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))

	res := c.get("/invoices/new")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `>acme</option>`)
	res.wantBody(t, `name="from" value="2026-09-01"`)
	res.wantBody(t, `name="to" value="2026-09-30"`)
	res.wantBody(t, `name="payment_days" value="30"`)
	res.wantBody(t, `<option value="15" selected>`)
}

func TestInvoiceConfigDefaults(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/invoices/new")

	res.wantBody(t, `name="payment_days" value="14"`)
	res.wantBody(t, `<option value="0" selected>`)
}

func TestInvalidInvoiceConfigFailsStartup(t *testing.T) {
	for _, toml := range []string{"rounding_minutes = 7", "rounding_minutes = -5", "payment_days = 0", "payment_days = -1"} {
		t.Run(toml, func(t *testing.T) {
			key, _, _ := strings.Cut(toml, " ")
			wantStartupError(t, devConfig+"[invoice]\n"+toml+"\n", "invoice."+key)
		})
	}
}

func TestDraftKeepsWhatItWasCreatedWith(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	form := draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30")
	form.Set("grouping", "entry")
	form.Set("payment_days", "21")

	draft := c.createDraft(form)

	res := c.get(draft)
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "acme")
	res.wantBody(t, `name="from" value="2026-09-01"`)
	res.wantBody(t, `name="to" value="2026-09-30"`)
	res.wantBody(t, `name="service_from" value=""`)
	res.wantBody(t, `name="service_to" value=""`)
	res.wantBody(t, `name="payment_days" value="21"`)
	res.wantBody(t, "One line per entry")
	res.wantBody(t, "15 minutes")
}

func TestDraftCreationIsValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	for field, tc := range map[string]struct{ value, message string }{
		"customer":         {"999", "Choose a customer."},
		"from":             {"2026-13-01", "Enter a valid date range."},
		"to":               {"2026-08-31", "Enter a valid date range."},
		"grouping":         {"customer", "Choose how lines are grouped."},
		"rounding_minutes": {"7", "Choose a rounding increment."},
		"payment_days":     {"0", "Payment days must be a positive whole number."},
	} {
		t.Run(field, func(t *testing.T) {
			form := draftForm(cust, "2026-09-01", "2026-09-30")
			form.Set(field, tc.value)

			res := c.postForm("/invoices", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
		})
	}
	for _, row := range c.get("/invoices").rows() {
		if slices.Contains(row, "acme") {
			t.Fatalf("refused draft listed: %q", row)
		}
	}
}

func TestArchivedCustomersCanBeInvoiced(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.postForm(cust+"/archive", nil).wantStatus(t, http.StatusSeeOther)

	c.get("/invoices/new").wantBody(t, `>acme</option>`)
	c.createDraft(draftForm(cust, "2026-09-01", "2026-09-30"))
}

func TestDraftHeaderStaysEditableButCustomerIsFixed(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	c.createCustomer(customerForm("globex", "90"))

	res := c.postForm(draft, url.Values{
		"customer":     {strings.TrimPrefix(c.customerPath("globex"), "/customers/")},
		"from":         {"2026-08-01"},
		"to":           {"2026-10-31"},
		"service_from": {"2026-08-15"},
		"service_to":   {"2026-10-15"},
		"payment_days": {"7"},
	})

	wantRedirect(t, res, draft)
	page := c.get(draft)
	page.wantBody(t, "acme")
	page.wantNoBody(t, "globex")
	page.wantBody(t, `name="from" value="2026-08-01"`)
	page.wantBody(t, `name="to" value="2026-10-31"`)
	page.wantBody(t, `name="service_from" value="2026-08-15"`)
	page.wantBody(t, `name="service_to" value="2026-10-15"`)
	page.wantBody(t, `name="payment_days" value="7"`)
}

func TestDraftHeaderUpdateIsValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()

	res := c.postForm(draft, url.Values{
		"from": {"2026-09-01"}, "to": {"2026-09-30"},
		"service_from": {"2026-09-30"}, "service_to": {"2026-09-01"},
		"payment_days": {"14"},
	})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Enter a valid service period.")
	res.wantBody(t, `name="service_from" value="2026-09-30"`)
	c.get(draft).wantBody(t, `name="service_from" value=""`)
}

func TestDraftServicePeriodIsEitherCompleteOrEmpty(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	header := url.Values{"from": {"2026-09-01"}, "to": {"2026-09-30"}, "service_from": {"2026-09-01"}, "service_to": {""}, "payment_days": {"14"}}

	c.postForm(draft, header).wantStatus(t, http.StatusUnprocessableEntity)
	header.Set("service_from", "")
	wantRedirect(t, c.postForm(draft, header), draft)

	page := c.get(draft)
	page.wantBody(t, `name="service_from" value=""`)
	page.wantBody(t, `name="service_to" value=""`)
}

func TestDraftCanBeDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()

	wantRedirect(t, c.postForm(draft+"/delete", nil), "/invoices")

	c.get(draft).wantStatus(t, http.StatusNotFound)
	c.get("/invoices").wantNoBody(t, `href="`+draft+`"`)
}

func lineForm(description, quantity, unitPrice string) url.Values {
	return url.Values{"description": {description}, "detail": {""}, "quantity": {quantity}, "unit": {"h"}, "unit_price": {unitPrice}}
}

func (c *client) addLine(draft string, form url.Values) {
	c.t.Helper()
	wantRedirect(c.t, c.postForm(draft+"/lines", form), draft)
}

var lineLink = regexp.MustCompile(`href="(/invoices/\d+/lines/\d+)"[^>]*>([^<]*)<`)

func (c *client) linePath(draft, description string) string {
	c.t.Helper()
	for _, m := range lineLink.FindAllStringSubmatch(c.get(draft).body, -1) {
		if m[2] == description {
			return m[1]
		}
	}
	c.t.Fatalf("no line %q on %s", description, draft)
	return ""
}

func TestLineTotalsRoundHalfUpAndSumToTheDraftTotal(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()

	c.addLine(draft, lineForm("Consulting", "1.333", "85"))
	c.addLine(draft, lineForm("Hosting", "2,5", "10.01"))

	res := c.get(draft)
	wantInOrder(t, res.body, "Consulting", "1.333", "85.00", "113.31", "Hosting", "2.5", "10.01", "25.03", "138.34", "138.34")
}

func TestStandardTaxModeAddsVATOnceOnTheNetTotal(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	draft := c.acmeDraft()

	c.addLine(draft, lineForm("Alpha", "1", "0.13"))
	c.addLine(draft, lineForm("Bravo", "1", "0.13"))

	res := c.get(draft)
	wantInOrder(t, res.body, "Net", "0.26", "VAT 19 %", "0.05", "Gross", "0.31")
	res = c.get("/invoices")
	res.wantBody(t, "0.31 EUR")
}

func TestSmallBusinessModeChargesNoVAT(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	form := businessForm()
	form.Set("tax_mode", "small_business")
	c.postForm("/settings/business", form).wantStatus(t, http.StatusSeeOther)
	draft := c.acmeDraft()

	c.addLine(draft, lineForm("Alpha", "1", "0.13"))

	res := c.get(draft)
	res.wantNoBody(t, "VAT")
	wantInOrder(t, res.body, "Net", "0.13", "Gross", "0.13")
}

func TestLinesAreValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	for name, tc := range map[string]struct {
		form    url.Values
		message string
	}{
		"no description": {lineForm("", "1", "85"), "Description is required."},
		"4 decimals":     {lineForm("Consulting", "1.0005", "85"), "Quantity must be a non-negative decimal with at most 3 decimals."},
		"price decimals": {lineForm("Consulting", "1", "85.001"), "Unit price must be a non-negative decimal with no more decimals than the currency allows."},
	} {
		t.Run(name, func(t *testing.T) {
			res := c.postForm(draft+"/lines", tc.form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
		})
	}
	c.get(draft).wantNoBody(t, "Consulting")
}

func TestLinesCanBeEditedReorderedAndDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	for _, d := range []string{"Alpha", "Bravo", "Charlie"} {
		c.addLine(draft, lineForm(d, "1", "10"))
	}

	edit := c.get(c.linePath(draft, "Bravo"))
	edit.wantBody(t, `name="description" value="Bravo"`)
	edit.wantBody(t, `name="quantity" value="1"`)
	edit.wantBody(t, `name="unit_price" value="10.00"`)
	form := lineForm("Bravo two", "3.25", "20")
	form.Set("detail", "September")
	form.Set("unit", "days")
	wantRedirect(t, c.postForm(c.linePath(draft, "Bravo"), form), draft)
	wantRedirect(t, c.postForm(c.linePath(draft, "Charlie")+"/up", nil), draft)
	wantRedirect(t, c.postForm(c.linePath(draft, "Alpha")+"/down", nil), draft)
	wantRedirect(t, c.postForm(c.linePath(draft, "Alpha")+"/down", nil), draft)

	res := c.get(draft)
	wantInOrder(t, res.body, "Charlie", "Bravo two", "September", "3.25", "days", "20.00", "65.00", "Alpha", "85.00")

	wantRedirect(t, c.postForm(c.linePath(draft, "Charlie")+"/delete", nil), draft)

	res = c.get(draft)
	res.wantNoBody(t, "Charlie")
	wantInOrder(t, res.body, "Bravo two", "Alpha", "75.00")
}

func TestAddLineLinkFollowsTheLines(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	addLine := `href="` + draft + `/lines/new"`

	c.get(draft).wantBody(t, addLine)

	c.addLine(draft, lineForm("Alpha", "1", "10"))
	wantInOrder(t, c.get(draft).body, "Alpha", addLine, "Net")
}

func TestLinesOfAnotherDraftAreNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	c.addLine(draft, lineForm("Alpha", "1", "10"))
	other := c.createDraft(draftForm(c.customerPath("acme"), "2026-08-01", "2026-08-31"))
	line := strings.TrimPrefix(c.linePath(draft, "Alpha"), draft)

	c.get(other+line).wantStatus(t, http.StatusNotFound)
	c.postForm(other+line+"/delete", nil).wantStatus(t, http.StatusNotFound)
	c.get(draft).wantBody(t, "Alpha")
}

func TestInvoiceListShowsDraftsNewestFirstAndFilters(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))
	acme := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))
	c.addLine(acme, lineForm("Consulting", "2", "85"))
	globex := c.createDraft(draftForm(c.customerPath("globex"), "2026-09-01", "2026-09-30"))

	res := c.get("/invoices")
	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.body, `href="`+globex+`"`, "globex", "Draft", `href="`+acme+`"`, "acme", "170.00 EUR", "Draft")

	byCustomer := c.get("/invoices?customer=" + strings.TrimPrefix(c.customerPath("acme"), "/customers/"))
	byCustomer.wantBody(t, `href="`+acme+`"`)
	byCustomer.wantNoBody(t, `href="`+globex+`"`)

	issued := c.get("/invoices?status=issued")
	issued.wantNoBody(t, `href="`+acme+`"`)
	issued.wantBody(t, "No invoices.")
	c.get("/invoices?status=draft").wantBody(t, `href="`+acme+`"`)
}

func TestCustomerWithAnInvoiceCannotBeDeletedOrChangeCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.acmeDraft()
	cust := c.customerPath("acme")

	res := c.postForm(cust+"/delete", nil)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This customer has invoices.")

	form := customerForm("acme", "85")
	form.Set("currency", "CHF")
	res = c.postForm(cust, form)
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "The currency of a customer with invoices can&#39;t change.")
	c.get(cust).wantBody(t, `name="currency" value="EUR"`)

	form.Set("currency", "EUR")
	form.Set("name", "acme corp")
	wantRedirect(t, c.postForm(cust, form), "/customers")
}

func TestCustomerIsDeletableOnceItsDraftsAreDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()
	cust := c.customerPath("acme")
	c.addLine(draft, lineForm("Alpha", "1", "10"))

	wantRedirect(t, c.postForm(draft+"/delete", nil), "/invoices")

	wantRedirect(t, c.postForm(cust+"/delete", nil), "/customers")
}
