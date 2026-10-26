package app_test

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"regexp"
	"strings"
	"testing"
	"time"
)

// hxPost sends a same-origin form submission as htmx does.
func (c *client) hxPost(path string, form url.Values) *response {
	c.t.Helper()
	req := httptest.NewRequest(http.MethodPost, baseURL+path, strings.NewReader(form.Encode()))
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Sec-Fetch-Site", "same-origin")
	req.Header.Set("HX-Request", "true")
	return c.do(req)
}

// websiteProject creates customer acme with project website and returns the project id.
func (c *client) websiteProject() string {
	c.t.Helper()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	return strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/")
}

func TestStartTimerOnAProjectFromTheHeader(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()

	c.get("/customers").wantBody(t, `<option value="`+project+`">website</option>`)
	res := c.hxPost("/timers", url.Values{"project": {project}, "description": {"homepage"}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `id="timers"`)
	res.wantNoBody(t, "<html")
	res.wantBody(t, "acme · website")
	res.wantBody(t, `value="homepage"`)
	page := c.get("/profile")
	page.wantBody(t, "acme · website")
	page.wantBody(t, "2026-10-26 09:00")
}

func TestStartTimerWithoutProjectIsUnassignedAndHasNoBillableFlag(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	res := c.hxPost("/timers", url.Values{"project": {""}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, "Unassigned")
	res.wantNoBody(t, "billable")
}

func TestTimerInheritsTheProjectsBillableDefault(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	internal := projectForm("internal", "")
	internal.Del("billable")
	c.createProject(cust, internal)
	c.createProject(cust, projectForm("website", ""))

	c.hxPost("/timers", url.Values{"project": {strings.TrimPrefix(c.projectPath(cust, "internal"), "/projects/")}})
	res := c.hxPost("/timers", url.Values{"project": {strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/")}})

	timers := strings.Split(res.body, `<div class="timer">`)[1:]
	if len(timers) != 2 {
		t.Fatalf("got %d timers:\n%s", len(timers), res.body)
	}
	for i, want := range []string{"acme · internal</span>", "acme · website</span>"} {
		if !strings.Contains(timers[i], want) {
			t.Errorf("timer %d lacks %q:\n%s", i, want, timers[i])
		}
	}
	if !strings.Contains(timers[0], `<span class="tag">not billable</span>`) || !strings.Contains(timers[1], `<span class="tag">billable</span>`) {
		t.Errorf("billable tags not inherited:\n%s", res.body)
	}
}

func TestSecondTimerIsRefusedWithParallelEntriesOff(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	project := c.websiteProject()
	c.hxPost("/timers", url.Values{"project": {project}}).wantStatus(t, http.StatusOK)

	res := c.hxPost("/timers", url.Values{"description": {"second"}})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "A timer is already running.")
	res.wantBody(t, `id="timers"`)
	res.wantNoBody(t, "Unassigned")
	plain := c.postForm("/timers", url.Values{})
	plain.wantStatus(t, http.StatusUnprocessableEntity)
	plain.wantBody(t, "<html")
	plain.wantBody(t, "A timer is already running.")
}

func TestParallelEntriesAllowSeveralTimers(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
parallel_entries = true
`)
	c := ta.client()

	c.hxPost("/timers", url.Values{"description": {"one"}}).wantStatus(t, http.StatusOK)
	res := c.hxPost("/timers", url.Values{"description": {"two"}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `value="one"`)
	res.wantBody(t, `value="two"`)
}

func TestAnotherUsersTimerDoesNotBlockOrShow(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	alice.hxPost("/timers", url.Values{"description": {"alice work"}}).wantStatus(t, http.StatusOK)

	res := bob.hxPost("/timers", url.Values{"description": {"bob work"}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `value="bob work"`)
	res.wantNoBody(t, "alice work")
}

var stopAction = regexp.MustCompile(`action="(/timers/\d+)/stop"`)

// startTimer starts a timer and returns its path.
func (c *client) startTimer(form url.Values) string {
	c.t.Helper()
	res := c.hxPost("/timers", form)
	res.wantStatus(c.t, http.StatusOK)
	m := stopAction.FindAllStringSubmatch(res.body, -1)
	if m == nil {
		c.t.Fatalf("no running timer:\n%s", res.body)
	}
	return m[len(m)-1][1]
}

func TestStoppingATimerEndsItAndFreesTheSlot(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	timer := c.startTimer(url.Values{"description": {"first"}})
	ta.clock.Advance(90 * time.Minute)

	res := c.hxPost(timer+"/stop", nil)

	res.wantStatus(t, http.StatusOK)
	res.wantNoBody(t, `aria-label="Stop"`)
	c.get("/").wantNoBody(t, `value="first"`)
	c.startTimer(url.Values{"description": {"second"}})
	c.hxPost(timer+"/stop", nil).wantStatus(t, http.StatusNotFound)
}

func TestStoppingWithoutHTMXRedirectsHome(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	timer := c.startTimer(url.Values{})

	res := c.postForm(timer+"/stop", nil)

	res.wantStatus(t, http.StatusSeeOther)
	if loc := res.Header.Get("Location"); loc != "/" {
		t.Fatalf("Location = %q, want /", loc)
	}
}

func TestOnlyTheOwnerCanStopOrEditATimer(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	timer := alice.startTimer(url.Values{"description": {"alice work"}})

	bob.hxPost(timer+"/stop", nil).wantStatus(t, http.StatusNotFound)
	bob.hxPost(timer, url.Values{"description": {"hijacked"}}).wantStatus(t, http.StatusNotFound)
	bob.hxPost("/timers/abc/stop", nil).wantStatus(t, http.StatusNotFound)

	alice.get("/").wantBody(t, `value="alice work"`)
}

func TestEditingARunningTimersDescription(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	timer := c.startTimer(url.Values{"description": {"draft"}})

	res := c.hxPost(timer, url.Values{"description": {" homepage redesign "}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, `value="homepage redesign"`)
	c.get("/").wantBody(t, `value="homepage redesign"`)
	c.hxPost(timer+"/stop", nil)
	c.hxPost(timer, url.Values{"description": {"late"}}).wantStatus(t, http.StatusNotFound)
}

func TestTimerSurvivesRestart(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")
	alice.startTimer(url.Values{"description": {"long haul"}})

	ta.restart()

	res := alice.get("/")
	res.wantBody(t, `value="long haul"`)
	res.wantBody(t, "since 2026-10-26 09:00")
}

func TestTimerRunningOver12HoursShowsABadgeAndKeepsRunning(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.startTimer(url.Values{"description": {"forgotten"}})

	ta.clock.Advance(12 * time.Hour)
	c.get("/").wantNoBody(t, "Running for more than 12 hours")
	ta.clock.Advance(time.Second)
	c.get("/").wantBody(t, "Running for more than 12 hours")
	ta.clock.Advance(30 * 24 * time.Hour)

	res := c.get("/customers")
	res.wantBody(t, `value="forgotten"`)
	res.wantBody(t, "Running for more than 12 hours")
}

func TestTimerHeaderInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig+`
[defaults]
language = "de"
`)
	c := ta.client()
	c.startTimer(url.Values{})

	res := c.hxPost("/timers", url.Values{})

	res.wantBody(t, "Es läuft bereits ein Timer.")
	res.wantBody(t, "Nicht zugeordnet")
	res.wantBody(t, "seit 26.10.2026 09:00")
}

func TestUnknownProjectIsRefused(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.hxPost("/timers", url.Values{"project": {"99"}}).wantBody(t, "This project can&#39;t receive new time.")
	res := c.hxPost("/timers", url.Values{"project": {"abc"}})

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantNoBody(t, `aria-label="Stop"`)
}
