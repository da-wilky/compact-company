package app_test

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

func customerForm(name, rate string) url.Values {
	return url.Values{
		"name":             {name},
		"address":          {"Hauptstr. 1\n12345 Berlin"},
		"email":            {"billing@" + name + ".example"},
		"currency":         {"EUR"},
		"invoice_language": {"en"},
		"timezone":         {"Europe/Berlin"},
		"hourly_rate":      {rate},
		"note":             {"met at conference"},
	}
}

func (c *client) createCustomer(form url.Values) {
	c.t.Helper()
	res := c.postForm("/customers", form)
	res.wantStatus(c.t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != "/customers" {
		c.t.Fatalf("Location = %q, want /customers", loc)
	}
}

func TestCreatedCustomersGetConsecutiveNumbers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.createCustomer(customerForm("acme", "85"))
	c.createCustomer(customerForm("globex", "90"))

	res := c.get("/customers")
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "K-001")
	res.wantBody(t, "acme")
	res.wantBody(t, "K-002")
	res.wantBody(t, "globex")
}

func TestCustomerNumberFormatIsConfigurable(t *testing.T) {
	ta := newTestApp(t, `customer_number_format = "C{seq:05}/x"`+devConfig)
	c := ta.client()

	c.createCustomer(customerForm("acme", "85"))

	c.get("/customers").wantBody(t, "C00001/x")
}

func TestCustomerNumbersOutgrowTheirWidth(t *testing.T) {
	ta := newTestApp(t, `customer_number_format = "K-{seq:02}"`+devConfig)
	c := ta.client()

	for range 100 {
		c.createCustomer(customerForm("acme", "85"))
	}

	res := c.get("/customers")
	res.wantBody(t, "K-99<")
	res.wantBody(t, "K-100<")
}

func TestCustomerNumberFormatWithoutSeqFailsStartup(t *testing.T) {
	wantStartupError(t, `customer_number_format = "K-"`+devConfig, "customer_number_format")
}

func TestCustomerNumbersContinueAfterRestart(t *testing.T) {
	ta := newTestApp(t, devConfig)
	ta.client().createCustomer(customerForm("acme", "85"))

	ta.restart()
	c := ta.client()
	c.createCustomer(customerForm("globex", "85"))

	res := c.get("/customers")
	res.wantBody(t, "K-001")
	res.wantBody(t, "K-002")
}

func TestFailedCreateConsumesNoNumber(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.postForm("/customers", customerForm("", "85")).wantStatus(t, http.StatusUnprocessableEntity)
	c.createCustomer(customerForm("acme", "85"))

	c.get("/customers").wantBody(t, "K-001")
}

func TestHourlyRateIsRequired(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.postForm("/customers", customerForm("acme", ""))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Default hourly rate is required.")
	res.wantBody(t, `value="acme"`)
	c.get("/customers").wantNoBody(t, "acme")
}

func TestHourlyRateUsesTheCurrencysMinorUnit(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	eur := customerForm("eurco", "85,5")
	jpy := customerForm("yenco", "8500")
	jpy.Set("currency", "jpy")
	bhd := customerForm("dinarco", "12.345")
	bhd.Set("currency", "BHD")

	c.createCustomer(eur)
	c.createCustomer(jpy)
	c.createCustomer(bhd)

	res := c.get("/customers")
	res.wantBody(t, "85.50 EUR")
	res.wantBody(t, "8500 JPY")
	res.wantBody(t, "12.345 BHD")
}

func TestHourlyRateWithTooManyDecimalsIsRejected(t *testing.T) {
	ta := newTestApp(t, devConfig)
	form := customerForm("yenco", "8500.5")
	form.Set("currency", "JPY")

	res := ta.client().postForm("/customers", form)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Default hourly rate must be")
}

func TestInvalidCustomerFieldsAreRejected(t *testing.T) {
	for field, want := range map[string]string{
		"name":             "Name is required.",
		"currency":         "Currency must be an ISO 4217 code",
		"invoice_language": "Unsupported invoice language.",
		"timezone":         "Unknown timezone.",
	} {
		t.Run(field, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			form := customerForm("acme", "85")
			form.Set(field, "")
			if field != "name" {
				form.Set(field, "XYZ/bogus")
			}

			res := ta.client().postForm("/customers", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, want)
		})
	}
}

func TestCustomerTimezoneLocalIsRejected(t *testing.T) {
	ta := newTestApp(t, devConfig)
	form := customerForm("acme", "85")
	form.Set("timezone", "Local")

	res := ta.client().postForm("/customers", form)

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Unknown timezone.")
}

func TestNewCustomerFormDefaultsFromConfig(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
currency = "CHF"
language = "de"
timezone = "Europe/Zurich"
`)

	res := ta.client().get("/customers/new")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `name="currency" value="CHF"`)
	res.wantBody(t, `<option value="de" selected>`)
	res.wantBody(t, `name="timezone" value="Europe/Zurich"`)
}

var customerLink = regexp.MustCompile(`<a href="(/customers/\d+)">([^<]*)</a>`)

func (c *client) customerPath(name string) string {
	c.t.Helper()
	for _, m := range customerLink.FindAllStringSubmatch(c.get("/customers").body, -1) {
		if m[2] == name {
			return m[1]
		}
	}
	c.t.Fatalf("no customer %q in list", name)
	return ""
}

func TestEditCustomerShowsStoredValues(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85.5"))

	res := c.get(c.customerPath("acme"))

	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.text(), "Edit customer", "K-001 acme")
	res.wantBody(t, `value="85.50"`)
	res.wantBody(t, "Hauptstr. 1\n12345 Berlin")
	res.wantBody(t, `value="billing@acme.example"`)
	res.wantBody(t, `value="Europe/Berlin"`)
	res.wantBody(t, "met at conference")
}

func TestAnyUserCanEditAnyCustomerIncludingCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	bob := ta.loginAs("bob")
	alice.createCustomer(customerForm("acme", "85"))
	path := bob.customerPath("acme")
	form := customerForm("acme corp", "12000")
	form.Set("currency", "JPY")

	res := bob.postForm(path, form)

	res.wantStatus(t, http.StatusSeeOther)
	list := alice.get("/customers")
	list.wantBody(t, "K-001")
	list.wantBody(t, "acme corp")
	list.wantBody(t, "12000 JPY")
}

func TestInvalidEditKeepsTheCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	path := c.customerPath("acme")

	res := c.postForm(path, customerForm("acme", ""))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	wantInOrder(t, res.text(), "Edit customer", "K-001 acme")
	c.get("/customers").wantBody(t, "85.00 EUR")
}

func TestUnknownCustomerIsNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/customers/99").wantStatus(t, http.StatusNotFound)
	c.get("/customers/abc").wantStatus(t, http.StatusNotFound)
	c.postForm("/customers/99", customerForm("acme", "85")).wantStatus(t, http.StatusNotFound)
	c.postForm("/customers/99", customerForm("", "85")).wantStatus(t, http.StatusNotFound)
}

func TestCustomerPagesInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
`)
	c := ta.client()

	c.get("/customers").wantBody(t, "Noch keine Kunden.")
	c.postForm("/customers", customerForm("acme", "")).wantBody(t, "Standard-Stundensatz ist erforderlich.")
	c.createCustomer(customerForm("acme", "85"))
	list := c.get("/customers")
	wantConfirmation(t, list.selectionQuestion(t, "Löschen", 1), "1 Kunde löschen?")
	wantConfirmation(t, list.selectionQuestion(t, "Löschen", 2), "2 Kunden löschen?")
}

func TestCrossOriginCustomerCreateIsRejected(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	req := httptest.NewRequest(http.MethodPost, baseURL+"/customers", strings.NewReader(customerForm("acme", "85").Encode()))
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Sec-Fetch-Site", "cross-site")

	c.do(req).wantStatus(t, http.StatusForbidden)
	c.get("/customers").wantNoBody(t, "acme")
}

func TestCurrencyChangeKeepsTheDecimalRate(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.createCustomer(customerForm("acme", "100.50"))
	path := c.customerPath("acme")
	form := customerForm("acme", "100.50")
	form.Set("currency", "JPY")

	c.postForm(path, form).wantStatus(t, http.StatusSeeOther)
	c.get("/customers").wantBody(t, "101 JPY")

	form = customerForm("acme", "101")
	form.Set("currency", "BHD")
	c.postForm(path, form).wantStatus(t, http.StatusSeeOther)
	c.get("/customers").wantBody(t, "101.000 BHD")
}
