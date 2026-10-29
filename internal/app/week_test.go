package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"slices"
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
	res.wantRow(t, "", "Mon 2026-10-26", "Tue 2026-10-27", "Wed 2026-10-28", "Thu 2026-10-29", "Fri 2026-10-30", "Sat 2026-10-31", "Sun 2026-11-01", "Total")
	res.wantBody(t, `href="/entries?day=2026-10-26">`)
	res.wantBody(t, `href="/entries?day=2026-11-01">`)
	res.wantRow(t, "website", "1:45", "", "0:30", "", "", "", "1:59", "4:15")
	res.wantBody(t, `href="/entries?day=2026-10-26&amp;project=`+project+`">1:45</a>`)
	res.wantBody(t, `href="/entries?day=2026-10-28&amp;project=`+project+`">0:30</a>`)
	res.wantBody(t, `href="/entries?day=2026-11-01&amp;project=`+project+`">1:59</a>`)
	res.wantRow(t, "Unassigned", "", "", "0:29", "", "", "", "", "0:29")
	res.wantBody(t, `href="/entries?day=2026-10-28&amp;project=none">0:29</a>`)
	res.wantRow(t, "Total", "1:45", "0:00", "1:00", "0:00", "0:00", "0:00", "1:59", "4:45")
}

func TestWeekGroupsProjectsUnderTheirCustomer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	website := c.websiteProject()
	c.createCustomer(customerForm("beta", "90"))
	beta := c.customerPath("beta")
	c.createProject(beta, projectForm("app", ""))
	c.createProject(beta, projectForm("hosting", ""))
	app := strings.TrimPrefix(c.projectPath(beta, "app"), "/projects/")
	hosting := strings.TrimPrefix(c.projectPath(beta, "hosting"), "/projects/")
	c.createEntry(entryForm(website, "2026-10-26T06:00", "2026-10-26T07:00", "acme"))
	c.createEntry(entryForm(app, "2026-10-26T07:00", "2026-10-26T08:00", "beta app"))
	c.createEntry(entryForm(hosting, "2026-10-26T08:00", "2026-10-26T09:00", "beta hosting"))

	res := c.get("/week")

	var firstCells []string
	for _, row := range res.rows() {
		firstCells = append(firstCells, row[0])
	}
	if want := []string{"", "acme", "website", "beta", "app", "hosting", "Unassigned", "Total"}; !slices.Equal(firstCells, want) {
		t.Fatalf("rows start with %q, want %q", firstCells, want)
	}
}

func TestWeekMarksTodayAndTheWeekend(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/week").wantBody(t, `<colgroup><col><col class="today"><col><col><col><col><col class="weekend"><col class="weekend"><col></colgroup>`)   // markup is the behaviour
	c.get("/week?week=2026-11-02").wantBody(t, `<colgroup><col><col><col><col><col><col><col class="weekend"><col class="weekend"><col></colgroup>`) // markup is the behaviour
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
	dst.wantRow(t, "Unassigned", "1:00", "", "", "", "", "", "1:15", "2:15")
	dst.wantBody(t, `href="/entries?day=2026-10-19&amp;project=none">1:00</a>`)
	dst.wantBody(t, `href="/entries?day=2026-10-25&amp;project=none">1:15</a>`)
	next.wantBody(t, "Week of 2026-10-26")
	next.wantRow(t, "Unassigned", "0:10", "", "", "", "", "", "", "0:10")
	next.wantBody(t, `href="/entries?day=2026-10-26&amp;project=none">0:10</a>`)
}

func TestWeekCountsRunningTimersUpToNow(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.startTimer(url.Values{})
	ta.clock.Advance(2*time.Hour + 30*time.Minute)

	c.get("/week").wantBody(t, `<a href="/entries?day=2026-10-26&amp;project=none">2:30</a>`)
	ta.clock.Advance(15 * time.Minute)
	c.get("/week").wantBody(t, `<a href="/entries?day=2026-10-26&amp;project=none">2:45</a>`)
}

func TestWeekNavigatesToPreviousAndNextWeek(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/week?week=2026-11-01")

	res.wantBody(t, `href="/week?week=2026-10-19">Previous week</a>`)
	res.wantBody(t, `href="/week?week=2026-11-02">Next week</a>`)
	wantInOrder(t, c.get("/").body, `href="/week"`, "Week</a>")
	c.get("/week?week=nope").wantStatus(t, http.StatusBadRequest)
}

func TestWeekOffersThisWeekOnlyWhenElsewhere(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/week").wantNoBody(t, "This week</a>")
	c.get("/week?week=2026-11-04&everyone=1").wantBody(t, `href="/week?everyone=1">This week</a>`)
	c.get("/week?week=2026-11-04").wantBody(t, `<input type="date" name="week" value="2026-11-02"`)
}

func TestWeekShowsOnlyMineUnlessEveryone(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
timezone = "Europe/Berlin"
`)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	alice.createEntry(entryForm("", "2026-10-27T00:30", "2026-10-27T01:00", "alice"))
	bob.createEntry(entryForm("", "2026-10-27T08:00", "2026-10-27T09:00", "bob"))
	bob.postForm("/profile", url.Values{"language": {"en"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	mine := bob.get("/week")
	everyone := bob.get("/week?everyone=1")

	mine.wantBody(t, `<a href="/entries?day=2026-10-27&amp;project=none">1:00</a>`)
	mine.wantNoBody(t, "0:30")
	mine.wantBody(t, `href="/week?everyone=1&amp;week=2026-10-26">Team</a>`)
	everyone.wantBody(t, `<a href="/entries?day=2026-10-26&amp;everyone=1&amp;project=none">0:30</a>`)
	everyone.wantBody(t, `<a href="/entries?day=2026-10-27&amp;everyone=1&amp;project=none">1:00</a>`)
	everyone.wantBody(t, `href="/week?everyone=1&amp;week=2026-11-02">Next week</a>`)
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
	self := "/entries?day=2026-10-26&amp;everyone=1&amp;project=" + project
	form := c.get(links[0][1])
	form.wantBody(t, `<input type="hidden" name="return" value="`+self+`">`)
	form.wantBody(t, `href="`+self+`"`)
	form.wantBody(t, "Time entries on 2026-10-26</a>")
	c.get("/entries/new").wantBody(t, "Time entries</a>")
	edit, del := links[1][2], links[0][2]
	c.get(edit+"?return=https://evil.test/entries").wantNoBody(t, `href="https://evil.test`)
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
	week.wantRow(t, "", "Mo 26.10.2026", "Di 27.10.2026", "Mi 28.10.2026", "Do 29.10.2026", "Fr 30.10.2026", "Sa 31.10.2026", "So 01.11.2026", "Summe")
	week.wantBody(t, `href="/week?week=2026-11-02">Nächste Woche</a>`)
	day.wantBody(t, "Zeiteinträge am 28.10.2026")
	day.wantBody(t, `href="/entries?day=2026-10-29">Nächster Tag</a>`)
}

func TestWeekAlwaysShowsTheUnassignedRow(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.get("/week")

	res.wantRow(t, "Unassigned", "", "", "", "", "", "", "", "0:00")
}

var editLinkWithReturn = regexp.MustCompile(`href="((/entries/\d+)\?return=[^"]+)"`)
