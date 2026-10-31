package app_test

import (
	"net/http"
	"net/url"
	"testing"
	"time"
)

func (c *client) sentQuote() string {
	c.t.Helper()
	quote := c.sendableQuote()
	wantRedirect(c.t, c.send(quote), quote)
	return quote
}

func (c *client) markOutcome(quote, outcome, date string) *response {
	c.t.Helper()
	return c.postForm(quote+"/"+outcome, url.Values{"date": {date}})
}

func TestMarkingASentQuoteAcceptedShowsTheDateOnItAndInTheList(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()

	wantRedirect(t, c.markOutcome(quote, "accept", "2026-11-03"), quote)

	res := c.get(quote)
	res.wantBody(t, "Accepted on 2026-11-03")
	res.wantNoBody(t, `action="`+quote+`/accept"`)
	c.get("/quotes").wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Accepted")
	c.get("/quotes?status=accepted").wantBody(t, `href="`+quote+`"`)
	c.get("/quotes?status=sent").wantNoBody(t, `href="`+quote+`"`)
}

func TestMarkingASentQuoteDeclinedShowsTheDateOnItAndInTheList(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()

	wantRedirect(t, c.markOutcome(quote, "decline", "2026-11-04"), quote)

	c.get(quote).wantBody(t, "Declined on 2026-11-04")
	c.get("/quotes").wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Declined")
	c.get("/quotes?status=declined").wantBody(t, `href="`+quote+`"`)
}

func TestTheOutcomeNeedsADateDefaultingToTodayInTheCustomersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()
	ta.clock.Advance(14*time.Hour + 30*time.Minute) // 00:30 on the 27th in Berlin
	c.get(quote).wantBody(t, `name="date" value="2026-10-27"`)

	res := c.markOutcome(quote, "accept", "03.11.2026")

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Enter a valid date.")
	res.wantBody(t, `name="date" value="03.11.2026"`)
	res.wantBody(t, "Quote A-2026-001")
	c.markOutcome(quote, "decline", "").wantStatus(t, http.StatusUnprocessableEntity)
	c.get(quote).wantBody(t, `action="`+quote+`/accept"`)
}

func TestUndoingTheOutcomeMakesTheQuoteSentAgain(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()
	wantRedirect(t, c.markOutcome(quote, "accept", "2026-11-03"), quote)

	wantRedirect(t, c.postForm(quote+"/undo", url.Values{}), quote)

	res := c.get(quote)
	res.wantNoBody(t, "Accepted on")
	res.wantBody(t, `action="`+quote+`/accept"`)
	c.get("/quotes").wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Sent")
	wantRedirect(t, c.markOutcome(quote, "decline", "2026-11-04"), quote)
	c.get(quote).wantBody(t, "Declined on 2026-11-04")
}

func TestOnlyAnUndecidedSentQuoteGetsAnOutcome(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()
	draft := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))
	wantRedirect(t, c.markOutcome(quote, "accept", "2026-11-03"), quote)

	for _, path := range []string{draft + "/accept", draft + "/decline", quote + "/accept", quote + "/decline"} {
		c.postForm(path, url.Values{"date": {"2026-11-05"}}).wantStatus(t, http.StatusNotFound)
	}
	c.postForm(draft+"/undo", url.Values{}).wantStatus(t, http.StatusNotFound)

	c.get(quote).wantBody(t, "Accepted on 2026-11-03")
	c.get(draft).wantBody(t, "Draft for K-001 acme")
}

func TestASentQuoteExpiresOnceItsValidityDateHasPassedInTheCustomersTimezone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	berlin := c.sentQuote()
	form := customerForm("initech", "85")
	form.Set("timezone", "America/Los_Angeles")
	c.createCustomer(form)
	la := c.createQuote(quoteForm(c.customerPath("initech"), "Portal"))
	wantRedirect(t, c.send(la), la)
	declined := c.createQuote(quoteForm(c.customerPath("acme"), "Hosting"))
	wantRedirect(t, c.send(declined), declined)
	wantRedirect(t, c.markOutcome(declined, "decline", "2026-11-02"), declined)
	ta.clock.Advance(30*24*time.Hour + 13*time.Hour + 59*time.Minute) // 23:59 on the 25th, the last valid day, in Berlin
	c.get("/quotes").wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Sent")

	ta.clock.Advance(time.Minute) // the 26th in Berlin, still the 25th in Los Angeles

	res := c.get("/quotes")
	res.wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Expired")
	res.wantRow(t, "A-2026-002", "initech", "Portal", "2026-10-26", "0.00 EUR", "Sent")
	res.wantRow(t, "A-2026-003", "acme", "Hosting", "2026-10-26", "0.00 EUR", "Declined")
	expired := c.get("/quotes?status=expired")
	expired.wantBody(t, `href="`+berlin+`"`)
	expired.wantNoBody(t, `href="`+la+`"`)
	expired.wantNoBody(t, `href="`+declined+`"`)
	c.get("/quotes?status=sent").wantBody(t, `href="`+la+`"`)
	c.get(berlin).wantBody(t, "Expired")
	c.get(la).wantNoBody(t, "Expired")
}

func TestAnExpiredQuoteCanStillBeAcceptedAfterAWarning(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.sentQuote()
	res := c.get(quote)
	res.wantNoBody(t, "A late acceptance is a new offer")
	res.wantNoBody(t, "has expired")
	ta.clock.Advance(31 * 24 * time.Hour)
	res = c.get(quote)
	res.wantBody(t, "A late acceptance is a new offer from the customer: mark it accepted only if you agree to it.")
	res.wantBody(t, `data-confirm="Quote A-2026-001 has expired. Mark it accepted anyway?"`) // markup is the behaviour: the warning step
	res.wantBody(t, `action="`+quote+`/accept"`)

	wantRedirect(t, c.markOutcome(quote, "accept", "2026-11-27"), quote)

	res = c.get(quote)
	res.wantBody(t, "Accepted on 2026-11-27")
	res.wantNoBody(t, "A late acceptance is a new offer")
	c.get("/quotes").wantRow(t, "A-2026-001", "acme", "Website relaunch", "2026-10-26", "1190.00 EUR", "Accepted")
}
