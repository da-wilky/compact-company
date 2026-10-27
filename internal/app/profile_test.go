package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func TestProfilePageIsLinkedFromTheLayout(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	alice := ta.loginAs("alice")

	alice.get("/").wantBody(t, `href="/profile"`)
	res := alice.get("/profile")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `href="/profile"`)
	res.wantBody(t, `<option value="" selected>Default (English)</option>`)
	res.wantBody(t, `name="timezone" value=""`)
	res.wantBody(t, `placeholder="Default (Europe/Berlin)"`)
}

func TestChangingLanguageAffectsOnlyThatUser(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")

	res := alice.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}})

	res.wantStatus(t, http.StatusSeeOther)
	alice.get("/").wantBody(t, "Angemeldet als alice")
	alice.get("/profile").wantBody(t, `<option value="de" selected>`)
	bob.get("/").wantBody(t, "Signed in as bob")
}

func TestUnsupportedLanguageIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")

	res := alice.postForm("/profile", url.Values{"language": {"fr"}, "timezone": {"UTC"}})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Unsupported language.")
	alice.get("/").wantBody(t, "Signed in as alice")
}

func TestTimezoneMustBeAnIANAName(t *testing.T) {
	for _, tz := range []string{"Local", "Mars/Olympus", "CEST", "+02:00", "../etc/passwd"} {
		t.Run(tz, func(t *testing.T) {
			ta := newTestApp(t, devConfig)
			alice := ta.loginAs("alice")

			res := alice.postForm("/profile", url.Values{"language": {"en"}, "timezone": {tz}})

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, "Unknown timezone.")
			alice.get("/profile").wantBody(t, `name="timezone" value=""`)
		})
	}
}

func TestTimesAreShownInTheViewersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")

	alice.postForm("/profile", url.Values{"language": {"en"}, "timezone": {"America/New_York"}}).wantStatus(t, http.StatusSeeOther)
	bob.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"Europe/Berlin"}}).wantStatus(t, http.StatusSeeOther)

	alice.get("/profile").wantBody(t, "Current time: 2026-10-26 05:00")
	bob.get("/profile").wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 10:00")
}

func TestLanguageAndTimezonePersistAcrossRestart(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"Asia/Tokyo"}}).wantStatus(t, http.StatusSeeOther)

	ta.restart()

	res := alice.get("/profile")
	res.wantBody(t, `value="Asia/Tokyo"`)
	res.wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 18:00")
}

func TestNewUserStartsWithConfiguredDefaultTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Asia/Kolkata"
`)

	ta.loginAs("alice").get("/profile").wantBody(t, "Current time: 2026-10-26 14:30")
}

func TestUsersFollowTheDefaultLanguageAndTimezoneUntilTheyChooseTheirOwn(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	wantRedirect(t, bob.postForm("/profile", url.Values{"language": {"en"}, "timezone": {"Asia/Tokyo"}}), "/profile")

	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("de", "Europe/Berlin", "EUR", false)), "/settings")

	alice.get("/profile").wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 10:00")
	bob.get("/profile").wantBody(t, "Current time: 2026-10-26 18:00")
	wantRedirect(t, bob.postForm("/profile", url.Values{"language": {""}, "timezone": {""}}), "/profile")
	res := bob.get("/profile")
	res.wantBody(t, "Aktuelle Uhrzeit: 26.10.2026 10:00")
	res.wantBody(t, `<option value="" selected>Voreinstellung (Deutsch)</option>`)
}

func TestNewUsersCopyTheParallelEntriesDefault(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("en", "UTC", "EUR", true)), "/settings")
	carol := ta.loginAs("carol")

	wantRedirect(t, alice.postForm("/settings/defaults", defaultsForm("en", "UTC", "EUR", false)), "/settings")

	alice.get("/profile").wantBody(t, `<input type="checkbox" name="parallel_entries">`)
	carol.get("/profile").wantBody(t, `<input type="checkbox" name="parallel_entries" checked>`)
}
