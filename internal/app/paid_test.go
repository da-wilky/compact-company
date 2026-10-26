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

func TestMarkingPaidIsRefusedWhenEveryLineIsCancelled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuedWorkshopAndHosting()
	c.cancelLines(invoice, "", "Workshop", "Hosting")
	c.get(invoice).wantNoBody(t, "Mark paid")

	res := c.markPaid(invoice, "2026-11-03")

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Every line of this invoice is cancelled, so it can&#39;t be marked paid.")
	c.get(invoice).wantNoBody(t, "Paid on")
}

func TestAPartiallyCancelledInvoiceCanStillBeMarkedPaid(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuedWorkshopAndHosting()
	c.cancelLines(invoice, "", "Hosting")

	wantRedirect(t, c.markPaid(invoice, "2026-11-03"), invoice)

	res := c.get(invoice)
	res.wantBody(t, "Paid on 2026-11-03")
	res.wantBody(t, "Partially cancelled")
}

func TestAPaidInvoiceCanStillBeCancelled(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	invoice := c.issuedWorkshopAndHosting()
	wantRedirect(t, c.markPaid(invoice, "2026-11-03"), invoice)

	cancellation := c.cancelLines(invoice, "refunded", "Workshop", "Hosting")

	c.get(cancellation).wantBody(t, "Cancellation invoice 2026-002")
	res := c.get(invoice)
	res.wantBody(t, "K-001 acme · Cancelled")
	res.wantBody(t, "Paid on 2026-11-03")
}

func TestCancellationInvoicesCantBeMarkedPaid(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	cancellation := c.cancelLines(c.issuedWorkshopAndHosting(), "", "Workshop")

	c.markPaid(cancellation, "2026-11-03").wantStatus(t, http.StatusNotFound)

	c.get(cancellation).wantNoBody(t, "Paid on")
	c.get("/invoices?status=paid").wantBody(t, "No invoices.")
}
