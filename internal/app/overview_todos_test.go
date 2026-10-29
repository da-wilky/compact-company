package app_test

import (
	"fmt"
	"net/http"
	"net/url"
	"regexp"
	"testing"
	"time"
)

// userID reads a user's id from the Todos page's assignee filter.
func (c *client) userID(name string) string {
	c.t.Helper()
	m := regexp.MustCompile(`<option value="(\d+)">` + regexp.QuoteMeta(name) + `</option>`).FindStringSubmatch(c.get("/todos").body)
	if m == nil {
		c.t.Fatalf("no user %q", name)
	}
	return m[1]
}

func assignedTodo(title, assignee string) url.Values {
	form := todoForm(title)
	form.Set("assignee", assignee)
	return form
}

func TestOverviewListsMyNotDoneTodosOfActiveProjects(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice, bob, carol := ta.loginAs("alice"), ta.loginAs("bob"), ta.loginAs("carol")
	website := alice.websitePath()
	acme := alice.customerPath("acme")
	alice.createProject(acme, projectForm("legacy", ""))
	legacy := alice.projectPath(acme, "legacy")
	aliceID, bobID := alice.userID("alice"), alice.userID("bob")
	alice.createTodo(website, assignedTodo("Write copy", aliceID))
	alice.createTodo(website, assignedTodo("Fix footer", ""))
	alice.createTodo(website, assignedTodo("Fix cart", bobID))
	alice.createTodo(website, assignedTodo("Ship it", aliceID))
	alice.postForm(alice.todoPath(website, "Ship it")+"/status", url.Values{"status": {statusID(t, alice.get(website), "Done")}}).wantStatus(t, http.StatusSeeOther)
	alice.createTodo(legacy, assignedTodo("Migrate data", aliceID))
	wantRedirect(t, alice.postForm(legacy+"/archive", nil), acme+"/projects")

	res := alice.get("/")

	mine := res.in(t, "My todos")
	wantCards(t, mine, "Write copy")
	mine.wantRow(t, "Write copy", "website", "Normal", "")
	res.wantNoBody(t, "Nothing to bill, nothing open.")
	wantCards(t, bob.get("/").in(t, "My todos"), "Fix cart")
	none := carol.get("/")
	none.wantNoBody(t, "My todos")
	none.wantBody(t, "Nothing to bill, nothing open.")
}

func TestOverviewOrdersMyTodosOverdueFirstThenByDueDatePriorityAndAge(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.loginAs("alice")
	project := c.websitePath()
	me := c.userID("alice")
	todo := func(title, priority, due string) {
		form := assignedTodo(title, me)
		form.Set("priority", priority)
		form.Set("due_date", due)
		c.createTodo(project, form)
		ta.clock.Advance(time.Minute)
	}
	todo("normal undated old", "1", "")
	todo("high undated", "2", "")
	todo("low later", "0", "2026-11-05")
	todo("high later", "2", "2026-11-05")
	todo("normal yesterday", "1", "2026-10-25")
	todo("low long overdue", "0", "2026-10-20")
	todo("normal today", "1", "2026-10-26")
	todo("normal undated new", "1", "")

	mine := c.get("/").in(t, "My todos")

	wantCards(t, mine, "low long overdue", "normal yesterday", "normal today", "high later", "low later",
		"high undated", "normal undated old", "normal undated new")
	mine.wantRow(t, "low long overdue", "website", "Low", "2026-10-20 Overdue")
	mine.wantRow(t, "normal yesterday", "website", "Normal", "2026-10-25 Overdue")
	mine.wantRow(t, "normal today", "website", "Normal", "2026-10-26")
}

func TestOverviewShowsTenOfMyTodosAndLinksToAllOfThem(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.loginAs("alice")
	project := c.websitePath()
	me := c.userID("alice")
	var titles []string
	for i := 1; i <= 11; i++ {
		title := fmt.Sprintf("Todo %02d", i)
		titles = append(titles, title)
		c.createTodo(project, assignedTodo(title, me))
		ta.clock.Advance(time.Minute)
	}

	mine := c.get("/").in(t, "My todos")

	wantCards(t, mine, titles[:10]...)
	mine.wantBody(t, `<a href="/todos?assignee=me">All my todos</a>`)
	wantCards(t, c.get("/todos?assignee=me").column(t, "Open"), titles...)
}

func TestOverviewShowsMyTodosInGerman(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.loginAs("alice")
	project := c.websitePath()
	form := assignedTodo("Texte schreiben", c.userID("alice"))
	form.Set("priority", "2")
	form.Set("due_date", "2026-10-25")
	c.createTodo(project, form)
	c.postForm("/profile", url.Values{"language": {"de"}, "timezone": {"UTC"}}).wantStatus(t, http.StatusSeeOther)

	mine := c.get("/").in(t, "Meine Aufgaben")

	mine.wantRow(t, "Titel", "Projekt", "Priorität", "Fälligkeitsdatum")
	mine.wantRow(t, "Texte schreiben", "website", "Hoch", "25.10.2026 Überfällig")
	mine.wantBody(t, "Alle meine Aufgaben")
}
