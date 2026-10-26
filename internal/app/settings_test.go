package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func defaultsForm(language, timezone, currency string, parallel bool) url.Values {
	form := url.Values{"language": {language}, "timezone": {timezone}, "currency": {currency}}
	if parallel {
		form.Set("parallel_entries", "on")
	}
	return form
}

func TestDefaultsAreEditedOnTheSettingsPageAndPrefillNewCustomers(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	wantRedirect(t, bob.postForm("/profile", url.Values{"language": {"en"}}), "/profile")

	alice.get("/").wantBody(t, `href="/settings"`)
	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("de", "Europe/Berlin", "CHF", true)), "/settings")

	page := bob.get("/settings").in(t, "Defaults")
	page.wantBody(t, `<option value="de" selected>`)
	page.wantBody(t, `name="timezone" value="Europe/Berlin"`)
	page.wantBody(t, `name="currency" value="CHF"`)
	page.wantBody(t, `name="parallel_entries" checked`)
	customer := bob.get("/customers/new")
	customer.wantBody(t, `name="currency" value="CHF"`)
	customer.wantBody(t, `<option value="de" selected>`)
	customer.wantBody(t, `name="timezone" value="Europe/Berlin"`)
}

func TestDefaultsAreValidated(t *testing.T) {
	for field, tc := range map[string]struct{ value, message string }{
		"language": {"fr", "Unsupported language."},
		"timezone": {"Local", "Unknown timezone."},
		"currency": {"EURO", "Currency must be an ISO 4217 code"},
	} {
		t.Run(field, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			alice := ta.loginAs("alice")
			form := defaultsForm("de", "Europe/Berlin", "CHF", false)
			form.Set(field, tc.value)

			res := alice.postForm("/settings/defaults", form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
			alice.get("/customers/new").wantBody(t, `name="currency" value="EUR"`)
		})
	}
}

func TestDefaultsSetInConfigAreReadOnly(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Asia/Kolkata"
`)
	alice := ta.loginAs("alice")

	page := alice.get("/settings").in(t, "Defaults")
	page.wantBody(t, `name="timezone" value="Asia/Kolkata" disabled`)
	page.wantBody(t, "Set in the configuration file")
	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("de", "Europe/Berlin", "CHF", false)), "/settings")

	alice.get("/customers/new").wantBody(t, `name="timezone" value="Asia/Kolkata"`)
	alice.get("/customers/new").wantBody(t, `name="currency" value="CHF"`)
}

func TestEveryDefaultSetInConfigIgnoresPostedChanges(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
timezone = "Asia/Kolkata"
currency = "CHF"
parallel_entries = true
`)
	alice := ta.loginAs("alice")
	page := alice.get("/settings").in(t, "Voreinstellungen")
	page.wantBody(t, `<select name="language" disabled>`)
	page.wantBody(t, `name="currency" value="CHF" disabled`)
	page.wantBody(t, `name="parallel_entries" checked disabled`)

	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("en", "UTC", "EUR", false)), "/settings")

	customer := alice.get("/customers/new")
	customer.wantBody(t, `<option value="de" selected>`)
	customer.wantBody(t, `name="timezone" value="Asia/Kolkata"`)
	customer.wantBody(t, `name="currency" value="CHF"`)
	ta.loginAs("bob").get("/profile").wantBody(t, `name="parallel_entries" checked`)
}

func TestPagesBeforeLoginUseTheStoredDefaultLanguage(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice"))
	alice := ta.client()
	alice.oidcLogin(iss, "alice", "Alice", "alice@example.com")

	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("de", "UTC", "EUR", false)), "/settings")

	ta.client().get("/login").wantBody(t, "Anmelden")
}
