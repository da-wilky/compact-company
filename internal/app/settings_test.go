package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func TestSettingsPageIsLinkedFromTheLayoutAndShowsDefaults(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	alice := ta.loginAs("alice")

	alice.get("/").wantBody(t, `href="/settings"`)
	res := alice.get("/settings")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `href="/settings"`)
	res.wantBody(t, `<option value="en" selected>`)
	res.wantBody(t, `value="Europe/Berlin"`)
}

func TestChangingLanguageAffectsOnlyThatUser(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")

	res := alice.postForm("/settings", url.Values{"language": {"de"}, "timezone": {"UTC"}})

	res.wantStatus(t, http.StatusSeeOther)
	alice.get("/").wantBody(t, "Angemeldet als alice")
	alice.get("/settings").wantBody(t, `<option value="de" selected>`)
	bob.get("/").wantBody(t, "Signed in as bob")
}

func TestUnsupportedLanguageIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")

	res := alice.postForm("/settings", url.Values{"language": {"fr"}, "timezone": {"UTC"}})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Unsupported language.")
	alice.get("/").wantBody(t, "Signed in as alice")
}

func TestTimezoneMustBeAnIANAName(t *testing.T) {
	for _, tz := range []string{"", "Local", "Mars/Olympus", "CEST", "+02:00", "../etc/passwd"} {
		t.Run(tz, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			alice := ta.loginAs("alice")

			res := alice.postForm("/settings", url.Values{"language": {"en"}, "timezone": {tz}})

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, "Unknown timezone.")
			alice.get("/settings").wantBody(t, `value="UTC"`)
		})
	}
}

func TestTimesAreShownInTheViewersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")

	alice.postForm("/settings", url.Values{"language": {"en"}, "timezone": {"America/New_York"}}).wantStatus(t, http.StatusSeeOther)
	bob.postForm("/settings", url.Values{"language": {"de"}, "timezone": {"Europe/Berlin"}}).wantStatus(t, http.StatusSeeOther)

	alice.get("/settings").wantBody(t, "Current time: 2026-10-26 05:00")
	bob.get("/settings").wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 10:00")
}

func TestLanguageAndTimezonePersistAcrossRestart(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.postForm("/settings", url.Values{"language": {"de"}, "timezone": {"Asia/Tokyo"}}).wantStatus(t, http.StatusSeeOther)

	ta.restart()

	res := alice.get("/settings")
	res.wantBody(t, `value="Asia/Tokyo"`)
	res.wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 18:00")
}

func TestNewUserStartsWithConfiguredDefaultTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Asia/Kolkata"
`)

	ta.loginAs("alice").get("/settings").wantBody(t, "Current time: 2026-10-26 14:30")
}
