package app_test

import (
	"net/http"
	"net/url"
	"testing"
	"time"
)

func TestOverviewIsTheStartPageInBothLanguages(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/")

	res.wantBody(t, "<title>Overview · compact-company</title>")
	wantInOrder(t, res.body, `href="/"`, "Overview</a>")
	c.postForm("/settings", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)
	de := c.get("/")
	de.wantBody(t, "<title>Übersicht · compact-company</title>")
	wantInOrder(t, de.body, `href="/"`, "Übersicht</a>")
	wantInOrder(t, de.text(), "Diese Woche 0.0 h · 0.0 h abrechenbar", "Dieser Monat 0.0 h", "Nichts abzurechnen, nichts offen.")
	c.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T06:30", "orphan"))
	c.get("/").wantBody(t, "1 nicht zugeordneter Eintrag (0.5 h)")
}

func TestOverviewHoursAreThisIsoWeekAndCalendarMonthByStartInMyTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	c := ta.client()
	c.createEntry(entryForm("", "2026-09-30T23:00", "2026-10-01T00:00", "september"))
	c.createEntry(entryForm("", "2026-10-01T00:00", "2026-10-01T00:30", "first of month"))
	c.createEntry(entryForm("", "2026-10-25T23:30", "2026-10-26T00:30", "sunday into monday"))
	c.createEntry(entryForm("", "2026-10-26T00:30", "2026-10-26T02:00", "monday"))
	c.createEntry(entryForm("", "2026-11-01T22:30", "2026-11-02T00:30", "november sunday"))
	c.createEntry(entryForm("", "2026-11-02T01:00", "2026-11-02T03:00", "next week"))

	res := c.get("/")

	wantInOrder(t, res.text(), "This week 3.5 h · 0.0 h billable", "This month 3.0 h · 0.0 h billable")
}

func TestOverviewHoursSplitOffTheBillablePartAndCountOnlyMyEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	project := alice.websiteProject()
	alice.createEntry(entryForm(project, "2026-10-26T06:00", "2026-10-26T08:00", "billable"))
	internal := entryForm(project, "2026-10-26T08:00", "2026-10-26T08:30", "internal")
	internal.Set("billable", "no")
	alice.createEntry(internal)
	alice.createEntry(entryForm("", "2026-10-05T08:00", "2026-10-05T09:00", "unassigned"))
	bob.createEntry(entryForm(project, "2026-10-26T06:00", "2026-10-26T08:00", "bob's"))
	alice.startTimer(url.Values{"project": {project}})
	ta.clock.Advance(30 * time.Minute)
	bob.startTimer(url.Values{})

	res := alice.get("/")

	wantInOrder(t, res.text(), "This week 3.0 h · 2.5 h billable", "This month 4.0 h · 2.5 h billable")
}

func TestOverviewCountsMyEndedUnassignedEntries(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	project := alice.websiteProject()
	alice.createEntry(entryForm("", "2026-08-03T08:00", "2026-08-03T09:00", "long ago"))
	alice.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T06:30", "today"))
	alice.createEntry(entryForm(project, "2026-10-26T07:00", "2026-10-26T08:00", "assigned"))
	bob.createEntry(entryForm("", "2026-10-26T06:00", "2026-10-26T08:00", "bob's"))
	alice.startTimer(url.Values{})
	ta.clock.Advance(time.Hour)

	res := alice.get("/")

	res.wantBody(t, `<a href="/entries?project=none">2 unassigned entries (1.5 h)</a>`)
	res.wantNoBody(t, "Nothing to bill, nothing open.")
	bob.get("/").wantBody(t, `<a href="/entries?project=none">1 unassigned entry (2.0 h)</a>`)
}

func TestOverviewWithNothingToDoSaysSo(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	internal := entryForm(project, "2026-10-26T07:00", "2026-10-26T08:00", "internal")
	internal.Set("billable", "no")
	c.createEntry(internal)
	c.startTimer(url.Values{})

	res := c.get("/")

	res.wantBody(t, "Nothing to bill, nothing open.")
	res.wantNoBody(t, "unassigned entr")
}

func TestOverviewShowsZeroHours(t *testing.T) {
	ta := newTestApp(t, devConfig)

	res := ta.client().get("/")

	wantInOrder(t, res.text(), "This week 0.0 h · 0.0 h billable", "This month 0.0 h · 0.0 h billable")
}
