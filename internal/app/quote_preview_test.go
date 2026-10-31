package app_test

import (
	"encoding/json"
	"net/http"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestQuotePreviewPassesTheContractToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	c.createCustomer(customerForm("acme", "85"))
	cust := c.customerPath("acme")
	c.createProject(cust, projectForm("website", ""))
	form := quoteForm(cust, "Website relaunch")
	form.Set("project", strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"))
	form.Set("language", "de")
	form.Set("pricing", "effort")
	quote := c.createQuote(form)
	workshop := quoteLineForm("Workshop", "1.5", "85", "once")
	workshop.Set("detail", "on site")
	c.addQuoteLine(quote, workshop)
	c.addQuoteLine(quote, quoteLineForm("Hosting", "2", "10.01", "monthly"))
	c.addQuoteLine(quote, quoteLineForm("Design", "1", "1000", "once"))
	c.addSection(quote, sectionForm("Scope", "We build *three* pages:\n\n- Home\n- About"))
	c.addSection(quote, sectionForm("", "Thanks for your interest."))
	ta.clock.Advance(14*time.Hour + 30*time.Minute) // 00:30 on the 27th in Berlin

	res := c.get(quote + "/preview")

	res.wantStatus(t, http.StatusInternalServerError)
	want := decodeJSON(t, `{
		"version": 1, "kind": "quote", "draft": true, "number": null,
		"issue_date": "2026-10-27", "valid_until": "2026-11-25",
		"subject": "Website relaunch", "pricing": "effort", "project": "website",
		"language": "de", "currency": "EUR",
		"business": {"name": "Samuel Wilk", "company": "swilk", "address": ["Hauptstraße 1", "12345 Berlin"],
			"email": "mail@swilk.example", "web": "swilk.example", "phone": "030 123", "tax_number": "12/345/67890",
			"bank": {"name": "GLS Bank", "iban": "DE02430609671234567890", "bic": "GENODEM1GLS"}},
		"tax": {"small_business": false, "vat_rate": 19},
		"customer": {"number": "K-001", "name": "acme", "address": ["Hauptstr. 1", "12345 Berlin"]},
		"lines": [
			{"description": "Workshop", "detail": "on site", "quantity": "1.5", "unit": "h", "unit_price": "85.00", "interval": "once", "total": "127.50"},
			{"description": "Hosting", "detail": "", "quantity": "2", "unit": "h", "unit_price": "10.01", "interval": "monthly", "total": "20.02"},
			{"description": "Design", "detail": "", "quantity": "1", "unit": "h", "unit_price": "1000.00", "interval": "once", "total": "1000.00"}
		],
		"sections": [
			{"heading": "Scope", "body": "We build *three* pages:\n\n- Home\n- About"},
			{"heading": "", "body": "Thanks for your interest."}
		],
		"totals": {
			"once": {"net": "1127.50", "vat": "214.23", "gross": "1341.73"},
			"monthly": {"net": "20.02", "vat": "3.80", "gross": "23.82"},
			"yearly": null
		}
	}`)
	if got := res.templateInput(t); !reflect.DeepEqual(got, want) {
		gotJSON, _ := json.MarshalIndent(got, "", "  ")
		t.Fatalf("template input:\n%s", gotJSON)
	}
}

func TestQuoteWithoutLinesOrSectionsPassesEmptyListsAndNoTotals(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	quote := c.acmeQuote()

	input := c.get(quote + "/preview").templateInput(t).(map[string]any)

	want := decodeJSON(t, `{"lines": [], "sections": [], "project": null, "totals": {"once": null, "monthly": null, "yearly": null}}`).(map[string]any)
	for k, w := range want {
		if !reflect.DeepEqual(input[k], w) {
			t.Errorf("%s = %v, want %v", k, input[k], w)
		}
	}
}

func TestQuotePreviewIsRefusedWhileBusinessDataIsMissing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()

	res := c.get(quote + "/preview")

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Invoices and quotes can&#39;t be previewed, issued or sent until these are set: Name, Address, Tax number")
}

func TestQuotePreviewNeedsQuoteTypInTheTemplateDirectory(t *testing.T) {
	ta := newTestApp(t, devConfig+"[invoice]\ntemplate_dir = "+strconv.Quote(t.TempDir())+"\n"+businessConfig)
	c := ta.client()
	quote := c.acmeQuote()

	res := c.get(quote + "/preview")

	res.wantStatus(t, http.StatusInternalServerError)
	res.wantBody(t, "The template directory has no quote.typ.")
}

func TestDefaultTemplatePreviewsAQuote(t *testing.T) {
	for _, tc := range []struct{ language, pricing, taxMode string }{
		{"en", "fixed", "standard"},
		{"de", "effort", "small_business"},
	} {
		for _, lines := range []bool{true, false} {
			t.Run(tc.language+"/"+tc.pricing+"/lines="+strconv.FormatBool(lines), func(t *testing.T) {
				ta := newTestApp(t, devConfig+strings.Replace(businessConfig, `"standard"`, strconv.Quote(tc.taxMode), 1))
				c := ta.client()
				c.createCustomer(customerForm("acme", "85"))
				form := quoteForm(c.customerPath("acme"), "Website relaunch")
				form.Set("language", tc.language)
				form.Set("pricing", tc.pricing)
				quote := c.createQuote(form)
				if lines {
					c.addQuoteLine(quote, quoteLineForm("Design", "1", "1000", "once"))
					c.addQuoteLine(quote, quoteLineForm("Hosting", "1", "20", "monthly"))
				}
				c.addSection(quote, sectionForm("Scope", "We build *three* pages:\n\n+ Home\n+ About\n\nWrite to info\\@acme.example."))
				c.addSection(quote, sectionForm("", "A paragraph without heading."))

				res := c.get(quote + "/preview")

				res.wantStatus(t, http.StatusOK)
				if ct := res.Header.Get("Content-Type"); ct != "application/pdf" {
					t.Fatalf("Content-Type = %q", ct)
				}
				if !strings.HasPrefix(res.body, "%PDF-") {
					t.Fatalf("body is not a PDF: %.40q", res.body)
				}
			})
		}
	}
}

func TestQuotePageLinksToItsPreview(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()

	c.get(quote).wantBody(t, `href="`+quote+`/preview"`)
}
