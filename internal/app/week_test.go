package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
	"time"
)

func TestWeekGroupsByProjectWithDailyAndWeeklyTotals(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.createEntry(entryForm(project, "2026-10-26T06:00", "2026-10-26T07:45", "monday"))
	c.createEntry(entryForm(project, "2026-10-28T06:00", "2026-10-28T06:30:40", "wednesday"))
	c.createEntry(entryForm("", "2026-10-28T07:00", "2026-10-28T07:29:30", "orphan"))
	c.createEntry(entryForm(project, "2026-11-01T22:00", "2026-11-01T23:59:59", "sunday"))
	c.createEntry(entryForm(project, "2026-11-02T06:00", "2026-11-02T07:00", "next monday"))

	res := c.get("/week?week=2026-10-28")

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "Week of 2026-10-26")
	res.wantBody(t, "<th>Mon<br>2026-10-26</th>")
	res.wantBody(t, "<th>Sun<br>2026-11-01</th>")
	res.wantBody(t, `<tr><th>acme · website</th><td class="num"><a href="/entries?day=2026-10-26&project=`+project+`">1:45</a></td><td class="num"></td><td class="num"><a href="/entries?day=2026-10-28&project=`+project+`">0:30</a></td><td class="num"></td><td class="num"></td><td class="num"></td><td class="num"><a href="/entries?day=2026-11-01&project=`+project+`">1:59</a></td><td class="num">4:15</td></tr>`)
	res.wantBody(t, `<tr><th>Unassigned</th><td class="num"></td><td class="num"></td><td class="num"><a href="/entries?day=2026-10-28&project=none">0:29</a></td>`)
	res.wantBody(t, `<tr><th>Total</th><td class="num">1:45</td><td class="num">0:00</td><td class="num">1:00</td><td class="num">0:00</td><td class="num">0:00</td><td class="num">0:00</td><td class="num">1:59</td><td class="num">4:45</td></tr>`)
}

func TestWeekIsMondayToSundayInTheViewersTimezoneAcrossDST(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	c := ta.client()
	c.createEntry(entryForm("", "2026-10-19T00:15", "2026-10-19T01:15", "first hour"))
	c.createEntry(entryForm("", "2026-10-25T23:30", "2026-10-26T00:45", "into monday"))
	c.createEntry(entryForm("", "2026-10-26T00:50", "2026-10-26T01:00", "monday"))

	dst := c.get("/week?week=2026-10-25")
	next := c.get("/week")

	dst.wantBody(t, "Week of 2026-10-19")
	dst.wantBody(t, `<tr><th>Unassigned</th><td class="num"><a href="/entries?day=2026-10-19&project=none">1:00</a></td>`)
	dst.wantBody(t, `<a href="/entries?day=2026-10-25&project=none">1:15</a></td><td class="num">2:15</td></tr>`)
	next.wantBody(t, "Week of 2026-10-26")
	next.wantBody(t, `<tr><th>Unassigned</th><td class="num"><a href="/entries?day=2026-10-26&project=none">0:10</a></td>`)
	next.wantBody(t, `<td class="num">0:10</td></tr>`)
}

func TestWeekCountsRunningTimersUpToNow(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.startTimer(url.Values{})
	ta.clock.Advance(2*time.Hour + 30*time.Minute)

	c.get("/week").wantBody(t, `<a href="/entries?day=2026-10-26&project=none">2:30</a>`)
	ta.clock.Advance(15 * time.Minute)
	c.get("/week").wantBody(t, `<a href="/entries?day=2026-10-26&project=none">2:45</a>`)
}

func TestWeekNavigatesToPreviousAndNextWeek(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/week?week=2026-11-01")

	res.wantBody(t, `href="/week?week=2026-10-19">Previous week</a>`)
	res.wantBody(t, `href="/week?week=2026-11-02">Next week</a>`)
	c.get("/").wantBody(t, `href="/week">Week</a>`)
	c.get("/week?week=nope").wantStatus(t, http.StatusBadRequest)
}

func TestWeekShowsOnlyMineUnlessEveryone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	alice.createEntry(entryForm("", "2026-10-27T00:30", "2026-10-27T01:00", "alice"))
	bob.createEntry(entryForm("", "2026-10-27T08:00", "2026-10-27T09:00", "bob"))
	bob.postForm("/settings", url.Values{"language": {"en"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	mine := bob.get("/week")
	everyone := bob.get("/week?everyone=1")

	mine.wantBody(t, `<a href="/entries?day=2026-10-27&project=none">1:00</a>`)
	mine.wantNoBody(t, "0:30")
	mine.wantBody(t, `href="/week?week=2026-10-26&everyone=1">Everyone</a>`)
	everyone.wantBody(t, `<a href="/entries?day=2026-10-26&project=none&everyone=1">0:30</a>`)
	everyone.wantBody(t, `<a href="/entries?day=2026-10-27&project=none&everyone=1">1:00</a>`)
	everyone.wantBody(t, `href="/week?week=2026-11-02&everyone=1">Next week</a>`)
	everyone.wantBody(t, `href="/week?week=2026-10-26">Mine</a>`)
}

func TestEditingFromADrilledInCellReturnsToIt(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.createEntry(entryForm(project, "2026-10-26T06:00", "2026-10-26T07:00", "first"))
	c.createEntry(entryForm(project, "2026-10-26T07:00", "2026-10-26T08:00", "second"))
	cell := "/entries?day=2026-10-26&project=" + project + "&everyone=1"

	links := editLinkWithReturn.FindAllStringSubmatch(c.get(cell).body, -1)
	if len(links) != 2 {
		t.Fatalf("edit links = %v", links)
	}
	c.get(links[0][1]).wantBody(t, `<input type="hidden" name="return" value="`+strings.ReplaceAll(cell, "&", "&amp;")+`">`)
	edit, del := links[0][2], links[1][2]
	update := entryForm(project, "2026-10-26T06:00", "2026-10-26T06:30", "first")
	update.Set("return", cell)
	wantRedirect(t, c.postForm(edit, update), cell)
	wantRedirect(t, c.postForm(del+"/delete", url.Values{"return": {cell}}), cell)
	update.Set("return", "https://evil.test/entries?day=2026-10-26")
	wantRedirect(t, c.postForm(edit, update), "/entries?day=2026-10-26")
}

func TestWeekAndDayDatesFollowTheLanguage(t *testing.T) {
	ta := newTestApp(t, germanDevConfig)
	c := ta.client()

	week := c.get("/week?week=2026-10-28")
	day := c.get("/entries?day=2026-10-28")

	week.wantBody(t, "Woche ab 26.10.2026")
	week.wantBody(t, "<th>Mo<br>26.10.2026</th>")
	week.wantBody(t, `href="/week?week=2026-11-02">Nächste Woche</a>`)
	day.wantBody(t, "Einträge am 28.10.2026")
	day.wantBody(t, `href="/entries?day=2026-10-29">Nächster Tag</a>`)
}

func TestWeekAlwaysShowsTheUnassignedRow(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/week")

	res.wantBody(t, `<tr><th>Unassigned</th><td class="num"></td>`)
}

var editLinkWithReturn =regexp.MustCompile(`href="((/entries/\d+)\?return=[^"]+)"`)
