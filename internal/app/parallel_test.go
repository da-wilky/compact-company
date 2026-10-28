package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func parallelSetting(on bool) url.Values {
	form := url.Values{"language": {"en"}, "timezone": {"UTC"}}
	if on {
		form.Set("parallel_entries", "on")
	}
	return form
}

func TestEnablingParallelEntriesAllowsSeveralTimersForThatUserOnly(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	alice.get("/profile").wantNoBody(t, `name="parallel_entries" checked`)

	alice.postForm("/profile", parallelSetting(true)).wantStatus(t, http.StatusSeeOther)

	alice.get("/profile").wantBody(t, `name="parallel_entries" checked`)
	alice.hxPost("/timers", url.Values{"description": {"one"}}).wantStatus(t, http.StatusOK)
	alice.hxPost("/timers", url.Values{"description": {"two"}}).wantStatus(t, http.StatusOK)
	bob.get("/profile").wantNoBody(t, `name="parallel_entries" checked`)
	bob.hxPost("/timers", url.Values{"description": {"one"}}).wantStatus(t, http.StatusOK)
	bob.hxPost("/timers", url.Values{"description": {"two"}}).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestOverlapsRemainAfterSwitchingOffAndAreCheckedOnNextSave(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.postForm("/profile", parallelSetting(true)).wantStatus(t, http.StatusSeeOther)
	alice.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T07:00", "one"))
	two := alice.createEntry(entryForm("", "2026-10-26T06:30", "2026-10-26T07:30", "two"))

	alice.postForm("/profile", parallelSetting(false)).wantStatus(t, http.StatusSeeOther)

	list := alice.get("/entries?day=2026-10-26")
	list.wantBody(t, "06:00–07:00")
	list.wantBody(t, "06:30–07:30")
	res := alice.postForm(two, entryForm("", "2026-10-26T06:30", "2026-10-26T07:30", "two edited"))
	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "This entry overlaps another of your entries.")
	alice.postForm(two, entryForm("", "2026-10-26T07:00", "2026-10-26T07:30", "two moved")).wantStatus(t, http.StatusSeeOther)
}

func TestNewUserStartsWithConfiguredParallelEntriesDefault(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)

	ta.loginAs("alice").get("/profile").wantBody(t, `name="parallel_entries" checked`)
}

func TestSwitchingParallelEntriesOffIsRefusedWhileSeveralTimersRun(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.postForm("/profile", parallelSetting(true)).wantStatus(t, http.StatusSeeOther)
	first := alice.startTimer(url.Values{"description": {"one"}})
	alice.startTimer(url.Values{"description": {"two"}})

	res := alice.postForm("/profile", parallelSetting(false))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Stop all but one of your timers before switching parallel entries off.")
	res.wantBody(t, `name="parallel_entries" checked`)
	alice.get("/profile").wantBody(t, `name="parallel_entries" checked`)
	alice.hxPost(first+"/stop", nil).wantStatus(t, http.StatusOK)
	alice.postForm("/profile", parallelSetting(false)).wantStatus(t, http.StatusSeeOther)
	alice.hxPost("/timers", url.Values{"description": {"three"}}).wantStatus(t, http.StatusUnprocessableEntity)
}
