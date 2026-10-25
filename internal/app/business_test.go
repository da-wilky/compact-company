package app_test

import (
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"testing"
)

func businessForm() url.Values {
	return url.Values{
		"name":       {"Samuel Wilk"},
		"company":    {"swilk"},
		"address":    {"Hauptstraße 1\n12345 Berlin"},
		"email":      {"mail@swilk.example"},
		"web":        {"swilk.example"},
		"phone":      {"030 123"},
		"tax_number": {"12/345/67890"},
		"bank_name":  {"GLS Bank"},
		"bank_iban":  {"DE02430609671234567890"},
		"bank_bic":   {"GENODEM1GLS"},
		"tax_mode":   {"standard"},
		"vat_rate":   {"19"},
	}
}

func TestBusinessDataIsEditedOnItsSettingsPage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")

	alice.get("/settings").wantBody(t, `href="/settings/business"`)
	alice.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	ta.restart()

	res := bob.get("/settings/business")
	res.wantStatus(t, http.StatusOK)
	for _, want := range []string{`value="Samuel Wilk"`, `value="swilk"`, "Hauptstraße 1\n12345 Berlin</textarea>",
		`value="mail@swilk.example"`, `value="swilk.example"`, `value="030 123"`, `value="12/345/67890"`,
		`value="GLS Bank"`, `value="DE02430609671234567890"`, `value="GENODEM1GLS"`,
		`<option value="standard" selected>`, `value="19"`} {
		res.wantBody(t, want)
	}
}

func TestBusinessTaxModeAndVATRateAreValidated(t *testing.T) {
	for field, value := range map[string]string{"tax_mode": "reverse_charge", "vat_rate": "19.5"} {
		t.Run(field, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			alice := ta.loginAs("alice")
			form := businessForm()
			form.Set(field, value)

			res := alice.postForm("/settings/business", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			alice.get("/settings/business").wantNoBody(t, `value="Samuel Wilk"`)
		})
	}
}

func TestBusinessPageNamesMissingRequiredFields(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")

	alice.get("/settings/business").wantBody(t, "Invoices can&#39;t be previewed or issued until these are set: Name, Address, Tax number")

	form := businessForm()
	form.Set("address", "  \n ")
	alice.postForm("/settings/business", form).wantStatus(t, http.StatusSeeOther)
	alice.get("/settings/business").wantBody(t, "until these are set: Address<")

	alice.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	alice.get("/settings/business").wantNoBody(t, "until these are set")
}

func writeBusinessFile(t *testing.T, content string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "business.toml")
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

const inlineBusiness = `
[business]
name = "Samuel Wilk"
address = ["Hauptstraße 1", "12345 Berlin"]
tax_number = "12/345/67890"
tax_mode = "standard"
vat_rate = 19

[business.bank]
name = "GLS Bank"
iban = "DE00INLINE"
`

func TestConfigManagedBusinessIsReadOnly(t *testing.T) {
	ta := newTestApp(t, devConfig+inlineBusiness)
	alice := ta.loginAs("alice")

	res := alice.get("/settings/business")
	res.wantBody(t, "Managed by config")
	res.wantBody(t, "<fieldset disabled>")
	res.wantBody(t, `value="Samuel Wilk"`)
	res.wantBody(t, "Hauptstraße 1\n12345 Berlin</textarea>")
	res.wantNoBody(t, "until these are set")

	form := businessForm()
	form.Set("name", "Mallory")
	res = alice.postForm("/settings/business", form)
	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "managed by config and can&#39;t be edited here")
	alice.get("/settings/business").wantNoBody(t, "Mallory")
}

func TestBusinessFileKeysOverrideInlineBusinessKeys(t *testing.T) {
	path := writeBusinessFile(t, `
tax_number = "99/999/99999"

[bank]
iban = "DE00SECRET"
`)
	ta := newTestApp(t, "business_file = "+strconv.Quote(path)+"\n"+devConfig+inlineBusiness)

	res := ta.loginAs("alice").get("/settings/business")
	res.wantBody(t, `value="Samuel Wilk"`)
	res.wantBody(t, `value="99/999/99999"`)
	res.wantBody(t, `value="GLS Bank"`)
	res.wantBody(t, `value="DE00SECRET"`)
	res.wantNoBody(t, "DE00INLINE")
}

func TestBusinessFileAloneMakesBusinessConfigManaged(t *testing.T) {
	path := writeBusinessFile(t, `name = "Samuel Wilk"`+"\n")
	ta := newTestApp(t, "business_file = "+strconv.Quote(path)+"\n"+devConfig)

	res := ta.loginAs("alice").get("/settings/business")
	res.wantBody(t, "Managed by config")
	res.wantBody(t, `value="Samuel Wilk"`)
	res.wantBody(t, "until these are set: Address, Tax number")
}

func TestInvalidBusinessConfigFailsStartup(t *testing.T) {
	wantStartupError(t, "business_file = \"/nonexistent/business.toml\"\n"+devConfig, "business_file")
	wantStartupError(t, "business_file = "+strconv.Quote(writeBusinessFile(t, "nme = \"x\"\n"))+"\n"+devConfig, "nme")
	wantStartupError(t, devConfig+"[business]\ntax_mode = \"reverse_charge\"\n", "business.tax_mode")
	wantStartupError(t, devConfig+"[business]\nvat_rate = 101\n", "business.vat_rate")
}
