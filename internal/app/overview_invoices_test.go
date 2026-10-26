package app_test

import (
	"net/http"
	"testing"
	"time"
)

func TestOverviewListsAnOpenInvoiceWithItsAmountDue(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	inv := c.issuableDraft()
	wantRedirect(t, c.issue(inv), inv)

	res := c.get("/")

	res.wantRow(t, "2026-001", "acme", "2026-11-09", "151.73 EUR")
	res.wantBody(t, `<a href="`+inv+`">2026-001</a>`)
	res.wantNoBody(t, "Nothing to bill, nothing open.")
}

func TestOverviewMarksInvoicesOverdueFromMidnightInTheCustomersTimezoneOldestDueFirst(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("acme", "85"))
	kiwi := customerForm("kiwi", "85")
	kiwi.Set("timezone", "Pacific/Auckland")
	c.createCustomer(kiwi)
	later := draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31")
	later.Set("payment_days", "30")
	c.createAndIssue(later, lineForm("Work", "1", "100"))
	c.createAndIssue(draftForm(c.customerPath("kiwi"), "2026-10-01", "2026-10-31"), lineForm("Work", "1", "100"))
	ta.clock.Advance(14*24*time.Hour + 1*time.Hour + 59*time.Minute) // 2026-11-09 23:59 in Auckland

	before := c.get("/")

	wantInOrder(t, before.text(), "2026-002 kiwi 2026-11-09 119.00 EUR", "2026-001 acme 2026-11-25 119.00 EUR")
	before.wantNoBody(t, "Overdue")
	ta.clock.Advance(time.Minute)
	after := c.get("/")
	after.wantRow(t, "2026-002", "kiwi", "2026-11-09 Overdue", "119.00 EUR")
	after.wantRow(t, "2026-001", "acme", "2026-11-25", "119.00 EUR")
}

func TestOverviewTotalsOpenAndOverdueAmountsPerCurrency(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	c.postForm("/settings/business", businessForm()).wantStatus(t, http.StatusSeeOther)
	c.createCustomer(customerForm("acme", "85"))
	yen := customerForm("yenco", "8500")
	yen.Set("currency", "JPY")
	c.createCustomer(yen)
	acme := draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31")
	c.createAndIssue(acme, lineForm("Old", "1", "100"))
	c.createAndIssue(draftForm(c.customerPath("yenco"), "2026-10-01", "2026-10-31"), lineForm("Work", "1", "1000"))
	ta.clock.Advance(15 * 24 * time.Hour)
	c.createAndIssue(acme, lineForm("New", "1", "10.50"))

	res := c.get("/")

	wantInOrder(t, res.text(), "Open in EUR 131.50 EUR · 119.00 EUR overdue", "Open in JPY 1190 JPY · 1190 JPY overdue")
}

func TestOverviewLeavesOutPaidCancelledAndCancellationInvoicesAndShowsWhatIsStillDue(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	partly := c.issuedWorkshopAndHosting()
	c.cancelLines(partly, "", "Hosting")
	acme := draftForm(c.customerPath("acme"), "2026-10-01", "2026-10-31")
	paid := c.createAndIssue(acme, lineForm("Paid", "1", "10"))
	wantRedirect(t, c.markPaid(paid, "2026-10-26"), paid)
	cancelled := c.createAndIssue(acme, lineForm("Cancelled", "1", "10"))
	c.cancelLines(cancelled, "", "Cancelled")
	paidPartly := c.createAndIssue(acme, lineForm("Kept", "1", "10"), lineForm("Dropped", "1", "10"))
	c.cancelLines(paidPartly, "", "Dropped")
	wantRedirect(t, c.markPaid(paidPartly, "2026-10-26"), paidPartly)
	draft := c.createDraft(acme)
	c.addLine(draft, lineForm("Draft", "1", "10"))

	open := c.get("/").in(t, "Open invoices")

	if rows := open.rows(); len(rows) != 2 {
		t.Fatalf("want a header and 2026-001 only; rows:\n%q", rows)
	}
	open.wantRow(t, "2026-001", "acme", "2026-11-09", "151.73 EUR")
}
