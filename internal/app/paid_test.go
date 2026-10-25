package app_test

import (
	"net/http"
	"net/url"
	"testing"
)

func (c *client) markPaid(invoice, date string) *response {
	c.t.Helper()
	return c.postForm(invoice+"/paid", url.Values{"paid_date": {date}})
}

func TestMarkingAnIssuedInvoicePaidShowsTheDateOnItAndInTheList(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuableDraft()
	wantRedirect(t, c.issue(invoice), invoice)

	wantRedirect(t, c.markPaid(invoice, "2026-11-03"), invoice)

	c.get(invoice).wantBody(t, "Paid on 2026-11-03")
	list := c.get("/invoices?status=paid")
	list.wantBody(t, "2026-001")
	list.wantBody(t, "2026-11-03")
	c.get("/invoices?status=issued").wantNoBody(t, "2026-001")
}

func TestMarkingPaidNeedsAPaymentDateDefaultingToToday(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuableDraft()
	wantRedirect(t, c.issue(invoice), invoice)
	c.get(invoice).wantBody(t, `name="paid_date" value="2026-10-26"`)

	res := c.markPaid(invoice, "03.11.2026")

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Enter a valid payment date.")
	res.wantBody(t, "Invoice 2026-001")
	c.get(invoice).wantNoBody(t, "Paid on")
}

func TestDraftsCantBeMarkedPaid(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.issuableDraft()

	c.markPaid(draft, "2026-11-03").wantStatus(t, http.StatusNotFound)
	c.postForm(draft+"/unpaid", url.Values{}).wantStatus(t, http.StatusNotFound)

	c.get(draft).wantBody(t, "Draft for acme")
	c.get("/invoices?status=paid").wantBody(t, "No invoices.")
}

func TestUndoingPaidMakesTheInvoiceIssuedAgain(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuableDraft()
	wantRedirect(t, c.issue(invoice), invoice)
	wantRedirect(t, c.markPaid(invoice, "2026-11-03"), invoice)

	wantRedirect(t, c.postForm(invoice+"/unpaid", url.Values{}), invoice)

	res := c.get(invoice)
	res.wantNoBody(t, "Paid on")
	res.wantBody(t, "Mark paid")
	c.get("/invoices?status=issued").wantBody(t, "2026-001")
	c.get("/invoices?status=paid").wantNoBody(t, "2026-001")
}
