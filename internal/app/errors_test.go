package app_test

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
)

const germanDevConfig = devConfig + `
[defaults]
language = "de"
`

func TestNotFoundPageIsTranslated(t *testing.T) {
	ta := newTestApp(t, germanDevConfig)
	c := ta.client()

	res := c.get("/customers/99")

	res.wantStatus(t, http.StatusNotFound)
	res.wantBody(t, `<html lang="de">`)
	res.wantBody(t, "Nicht gefunden.")
}

func TestBadRequestAndForbiddenPagesAreTranslated(t *testing.T) {
	ta := newTestApp(t, germanDevConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	entry := alice.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T07:00", "mine"))

	week := alice.get("/week?week=nope")
	day := alice.get("/entries?day=nope")
	project := alice.get("/entries?project=nope")
	del := bob.postForm(entry+"/delete", nil)

	week.wantStatus(t, http.StatusBadRequest)
	week.wantBody(t, "Ungültige Woche.")
	day.wantStatus(t, http.StatusBadRequest)
	day.wantBody(t, "Ungültiger Tag.")
	project.wantStatus(t, http.StatusBadRequest)
	project.wantBody(t, "Ungültiges Projekt.")
	del.wantStatus(t, http.StatusForbidden)
	del.wantBody(t, "Du darfst das nicht ändern.")
}

func TestCrossOriginRejectionIsTranslated(t *testing.T) {
	ta := newTestApp(t, germanDevConfig)
	req := httptest.NewRequest(http.MethodPost, baseURL+"/", nil)
	req.Header.Set("Sec-Fetch-Site", "cross-site")

	res := ta.client().do(req)

	res.wantStatus(t, http.StatusForbidden)
	res.wantBody(t, "Du darfst das nicht ändern.")
}

func TestUnknownRouteIsTranslatedInTheUsersLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.postForm("/settings", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	missing := alice.get("/nope")
	method := alice.postForm("/week", nil)

	missing.wantStatus(t, http.StatusNotFound)
	missing.wantBody(t, "Nicht gefunden.")
	missing.wantBody(t, "Angemeldet als alice")
	method.wantStatus(t, http.StatusMethodNotAllowed)
	method.wantBody(t, "Diese Aktion wird hier nicht unterstützt.")
}

func TestLoginFailurePageUsesTheDefaultLanguage(t *testing.T) {
	ta := newTestApp(t, oidcConfig(t)+`
[defaults]
language = "de"
`)

	res := ta.client().get("/auth/callback?state=x&code=y")

	res.wantStatus(t, http.StatusBadRequest)
	res.wantBody(t, `<html lang="de">`)
	res.wantBody(t, "Anmeldung fehlgeschlagen.")
}
