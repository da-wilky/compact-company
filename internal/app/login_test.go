package app_test

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
	"time"
)

func wantRedirect(t *testing.T, res *response, location string) {
	t.Helper()
	res.wantStatus(t, http.StatusSeeOther)
	if got := res.Header.Get("Location"); got != location {
		t.Fatalf("Location = %q, want %q", got, location)
	}
}

func TestAllowedSubjectLogsInThroughTheIssuer(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice"))
	c := ta.client()

	wantRedirect(t, c.oidcLogin(iss, "alice", "Alice Example", "alice@example.com"), "/")

	res := c.get("/")
	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "Signed in as Alice Example")
}

func TestSubjectNotAllowedSeesNotAllowedPageAndGetsNoSession(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice"))
	c := ta.client()

	res := c.oidcLogin(iss, "mallory", "Mallory", "mallory@example.com")

	res.wantStatus(t, http.StatusForbidden)
	res.wantBody(t, "not allowed")
	res.wantBody(t, "mallory")
	wantRedirect(t, c.get("/"), "/login")
}

func TestLoginPageOffersLoginInDefaultLanguage(t *testing.T) {
	ta := newTestApp(t, oidcConfig(t)+"\n[defaults]\nlanguage = \"de\"\n")

	res := ta.client().get("/login")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `<form method="post" action="/login">`)
	res.wantBody(t, "Anmelden")
}

func TestFirstLoginUsesConfigDefaultsAndLaterLoginsRefreshClaims(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice")+"\n[defaults]\nlanguage = \"de\"\n")
	laptop, phone := ta.client(), ta.client()

	laptop.oidcLogin(iss, "alice", "Alice", "alice@example.com")
	laptop.get("/").wantBody(t, "Angemeldet als Alice")

	phone.oidcLogin(iss, "alice", "Alice Renamed", "alice@new.example.com")

	laptop.get("/").wantBody(t, "Angemeldet als Alice Renamed")
}

func TestCallbackWithForeignStateIsRejected(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice"))
	victim, attacker := ta.client(), ta.client()

	victim.postForm("/login", nil)
	res := attacker.postForm("/login", nil)
	callback := iss.authorize(res.Header.Get("Location"), "alice", "Alice", "alice@example.com")

	victim.do(httptest.NewRequest(http.MethodGet, callback, nil)).wantStatus(t, http.StatusBadRequest)
	wantRedirect(t, victim.get("/"), "/login")
}

func TestLogoutEndsTheSessionServerSide(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice"))
	c := ta.client()
	c.oidcLogin(iss, "alice", "Alice", "alice@example.com")
	copied := ta.client()
	u, _ := url.Parse(baseURL)
	copied.jar.SetCookies(u, c.jar.Cookies(u))

	c.get("/").wantBody(t, `<form method="post" action="/logout">`)

	wantRedirect(t, c.postForm("/logout", nil), "/")

	wantRedirect(t, c.get("/"), "/login")
	wantRedirect(t, copied.get("/"), "/login")
}

func TestSessionOfSubjectRemovedFromConfigIsRejectedAfterRestart(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice", "bob"))
	alice, bob := ta.client(), ta.client()
	alice.oidcLogin(iss, "alice", "Alice", "alice@example.com")
	bob.oidcLogin(iss, "bob", "Bob", "bob@example.com")

	ta.cfg.Auth.AllowedSubjects = []string{"bob"}
	ta.restart()

	wantRedirect(t, alice.get("/"), "/login")
	bob.get("/").wantBody(t, "Signed in as Bob")
}

func TestOIDCSessionExpiresAfterIdleTimeout(t *testing.T) {
	iss := newIssuer(t)
	ta := newTestApp(t, iss.config(t, "alice")+`session_idle_timeout = "1h"`+"\n")
	c := ta.client()
	c.oidcLogin(iss, "alice", "Alice", "alice@example.com")

	ta.clock.Advance(61 * time.Minute)

	wantRedirect(t, c.get("/"), "/login")
}
