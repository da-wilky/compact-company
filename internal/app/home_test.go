package app_test

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestDevModeLandsOnHomeAsDevUser(t *testing.T) {
	ta := newTestApp(t, devConfig)

	res := ta.client().get("/")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "Signed in as dev")
}

func TestHomeUsesConfiguredDefaultLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
`)

	res := ta.client().get("/")

	res.wantBody(t, `<html lang="de">`)
	res.wantBody(t, "Angemeldet als dev")
}

func TestAcceptLanguageIsIgnored(t *testing.T) {
	ta := newTestApp(t, devConfig)
	req := httptest.NewRequest(http.MethodGet, baseURL+"/", nil)
	req.Header.Set("Accept-Language", "de-DE,de;q=0.9")

	res := ta.client().do(req)

	res.wantBody(t, "Signed in as dev")
}
