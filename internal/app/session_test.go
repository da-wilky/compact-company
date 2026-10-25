package app_test

import (
	"net/http"
	"testing"
	"time"
)

func TestSessionCookieIsSecureHttpOnlyLaxAndLastsTheIdleTimeout(t *testing.T) {
	ta := newTestApp(t, devConfig+`session_idle_timeout = "2h"`+"\n")

	res := ta.client().get("/")

	var session *http.Cookie
	for _, c := range res.Cookies() {
		if c.Name == "session" {
			session = c
		}
	}
	if session == nil {
		t.Fatal("no session cookie set")
	}
	if !session.Secure || !session.HttpOnly || session.SameSite != http.SameSiteLaxMode {
		t.Errorf("cookie Secure=%v HttpOnly=%v SameSite=%v, want true true Lax", session.Secure, session.HttpOnly, session.SameSite)
	}
	if session.MaxAge != 7200 {
		t.Errorf("cookie MaxAge = %d, want 7200", session.MaxAge)
	}
}

func TestLoginAsActsAsThatUser(t *testing.T) {
	ta := newTestApp(t, devConfig)

	res := ta.loginAs("alice").get("/")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "Signed in as alice")
}

func TestSessionExpiresAfterIdleTimeoutButActivityKeepsItAlive(t *testing.T) {
	ta := newTestApp(t, devConfig+`session_idle_timeout = "1h"`+"\n")
	alice := ta.loginAs("alice")

	ta.clock.Advance(59 * time.Minute)
	alice.get("/").wantBody(t, "Signed in as alice")
	ta.clock.Advance(59 * time.Minute)
	alice.get("/").wantBody(t, "Signed in as alice")
	ta.clock.Advance(61 * time.Minute)

	res := alice.get("/")

	res.wantNoBody(t, "Signed in as alice")
	res.wantBody(t, "Signed in as dev")
}

func wantSessions(t *testing.T, ta *testApp, want int) {
	t.Helper()
	n, err := ta.app.SessionCountForTest()
	if err != nil {
		t.Fatal(err)
	}
	if n != want {
		t.Fatalf("sessions = %d, want %d", n, want)
	}
}

func TestExpiredSessionsArePurgedWhenANewSessionStarts(t *testing.T) {
	ta := newTestApp(t, devConfig+`session_idle_timeout = "1h"`+"\n")
	ta.client().get("/")
	ta.client().get("/")
	ta.clock.Advance(61 * time.Minute)

	ta.client().get("/")

	wantSessions(t, ta, 1)
}

func TestExpiredSessionsArePurgedAtStartup(t *testing.T) {
	ta := newTestApp(t, devConfig+`session_idle_timeout = "1h"`+"\n")
	ta.client().get("/")
	ta.clock.Advance(30 * time.Minute)
	ta.client().get("/")
	ta.clock.Advance(31 * time.Minute)

	ta.restart()

	wantSessions(t, ta, 1)
}

func TestNewUserStartsWithConfiguredDefaultLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
`)

	ta.loginAs("alice").get("/").wantBody(t, "Angemeldet als alice")
}

func TestUnauthenticatedRequestInOIDCModeGoesToLogin(t *testing.T) {
	ta := newTestApp(t, oidcConfig(t))

	res := ta.client().get("/")

	res.wantStatus(t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != "/login" {
		t.Fatalf("Location = %q, want /login", loc)
	}
}
