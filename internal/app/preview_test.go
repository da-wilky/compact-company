package app_test

import (
	"encoding/json"
	"html"
	"net/http"
	"net/url"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

const businessConfig = `
[business]
name = "Samuel Wilk"
company = "swilk"
address = ["Hauptstraße 1", "12345 Berlin"]
email = "mail@swilk.example"
web = "swilk.example"
phone = "030 123"
tax_number = "12/345/67890"
bank = { name = "GLS Bank", iban = "DE02430609671234567890", bic = "GENODEM1GLS" }
tax_mode = "standard"
vat_rate = 19
`

// dumpConfig renders through a fixture template that fails with the JSON it received.
func dumpConfig(t *testing.T) string {
	t.Helper()
	dir, err := filepath.Abs("testdata/dump")
	if err != nil {
		t.Fatal(err)
	}
	return devConfig + "[invoice]\ntemplate_dir = " + strconv.Quote(dir) + "\nfont_paths = [" + strconv.Quote(dir) + "]\n" + businessConfig
}

// templateInput returns the JSON the dump template received, decoded.
func (r *response) templateInput(t *testing.T) any {
	t.Helper()
	_, after, ok := strings.Cut(r.body, "panicked with: ")
	if !ok {
		t.Fatalf("no template input in body:\n%s", r.body)
	}
	data, _, _ := strings.Cut(after, "\n")
	var v any
	if err := json.Unmarshal([]byte(html.UnescapeString(data)), &v); err != nil {
		t.Fatalf("template input is not JSON: %v\n%s", err, data)
	}
	return v
}

func decodeJSON(t *testing.T, s string) any {
	t.Helper()
	var v any
	if err := json.Unmarshal([]byte(s), &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestPreviewPassesTheContractToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	c := ta.client()
	form := customerForm("acme", "85")
	form.Set("invoice_language", "de")
	c.createCustomer(form)
	draft := c.createDraft(draftForm(c.customerPath("acme"), "2026-09-01", "2026-09-30"))
	workshop := lineForm("Workshop", "1.5", "85")
	workshop.Set("detail", "on site")
	c.addLine(draft, workshop)
	c.addLine(draft, lineForm("Hosting", "2", "10.01"))
	wantRedirect(t, c.postForm(draft, url.Values{"from": {""}, "to": {""}, "service_from": {"2026-09-01"}, "service_to": {"2026-09-30"}, "payment_days": {"14"}}), draft)
	ta.clock.Advance(14*time.Hour + 30*time.Minute) // 00:30 on the 27th in Berlin

	res := c.get(draft + "/preview")

	res.wantStatus(t, http.StatusInternalServerError)
	want := decodeJSON(t, `{
		"version": 1, "kind": "invoice", "draft": true, "number": null,
		"issue_date": "2026-10-27", "due_date": "2026-11-10",
		"service_period": {"from": "2026-09-01", "to": "2026-09-30"},
		"language": "de", "currency": "EUR",
		"business": {"name": "Samuel Wilk", "company": "swilk", "address": ["Hauptstraße 1", "12345 Berlin"],
			"email": "mail@swilk.example", "web": "swilk.example", "phone": "030 123", "tax_number": "12/345/67890",
			"bank": {"name": "GLS Bank", "iban": "DE02430609671234567890", "bic": "GENODEM1GLS"}},
		"tax": {"small_business": false, "vat_rate": 19},
		"customer": {"number": "K-001", "name": "acme", "address": ["Hauptstr. 1", "12345 Berlin"]},
		"lines": [
			{"description": "Workshop", "detail": "on site", "quantity": "1.5", "unit": "h", "unit_price": "85.00", "total": "127.50", "entries": [], "project": null},
			{"description": "Hosting", "detail": "", "quantity": "2", "unit": "h", "unit_price": "10.01", "total": "20.02", "entries": [], "project": null}
		],
		"totals": {"net": "147.52", "vat": "28.03", "gross": "175.55"},
		"cancels": null, "reason": ""
	}`)
	if got := res.templateInput(t); !reflect.DeepEqual(got, want) {
		gotJSON, _ := json.MarshalIndent(got, "", "  ")
		t.Fatalf("template input:\n%s", gotJSON)
	}
}

func TestEachLineCarriesItsEntriesToTheTemplate(t *testing.T) {
	ta := newTestApp(t, dumpConfig(t))
	alice, bob := ta.loginAs("alice"), ta.loginAs("bob")
	website, _ := alice.acmeProjects()
	alice.createEntry(entryForm(website, "2026-08-31T22:30", "2026-08-31T23:30", "first Berlin hour"))
	bob.createEntry(entryForm(website, "2026-09-10T08:00", "2026-09-10T08:15", "bob's quarter"))
	draft := alice.createDraft(pullForm(alice, "project", "15"))
	alice.addLine(draft, lineForm("Hosting", "1", "10"))

	res := alice.get(draft + "/preview")

	lines := res.templateInput(t).(map[string]any)["lines"].([]any)
	want := []any{
		decodeJSON(t, `[
			{"date": "2026-09-01", "user": "alice", "seconds": 3600, "description": "first Berlin hour"},
			{"date": "2026-09-10", "user": "bob", "seconds": 900, "description": "bob's quarter"}
		]`),
		decodeJSON(t, `[]`),
	}
	for i, w := range want {
		if got := lines[i].(map[string]any)["entries"]; !reflect.DeepEqual(got, w) {
			t.Errorf("lines[%d].entries = %v, want %v", i, got, w)
		}
	}
}

func (r *response) servicePeriod(t *testing.T) any {
	t.Helper()
	return r.templateInput(t).(map[string]any)["service_period"]
}

func TestServicePeriodRunsFromTheFirstEntrysStartToTheLastEntrysEnd(t *testing.T) {
	// Users enter UTC; acme is billed in Europe/Berlin, two hours ahead in September.
	for name, tc := range map[string]struct{ lastStart, lastEnd, wantTo string }{
		"past midnight":          {"2026-09-19T21:00", "2026-09-19T22:30", "2026-09-20"},
		"ending at midnight":     {"2026-09-19T20:00", "2026-09-19T22:00", "2026-09-19"},
		"ending before midnight": {"2026-09-19T08:00", "2026-09-19T09:00", "2026-09-19"},
	} {
		t.Run(name, func(t *testing.T) {
			ta := newTestApp(t, dumpConfig(t))
			c := ta.client()
			website, hosting := c.acmeProjects()
			c.createEntry(entryForm(hosting, tc.lastStart, tc.lastEnd, "last"))
			c.createEntry(entryForm(website, "2026-08-31T22:30", "2026-08-31T23:30", "first Berlin hour"))
			draft := c.createDraft(pullForm(c, "project", "15"))
			c.addLine(draft, lineForm("Hosting", "1", "10"))

			got := c.get(draft + "/preview").servicePeriod(t)

			if want := decodeJSON(t, `{"from": "2026-09-01", "to": "`+tc.wantTo+`"}`); !reflect.DeepEqual(got, want) {
				t.Fatalf("service_period = %v, want %v", got, want)
			}
		})
	}
}

func TestPreviewIsRefusedWhileBusinessDataIsMissing(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	draft := c.acmeDraft()

	res := c.get(draft + "/preview")

	res.wantStatus(t, http.StatusConflict)
	res.wantBody(t, "Invoices can&#39;t be previewed or issued until these are set: Name, Address, Tax number")
}

func TestDefaultTemplatePreviewsADraftWithoutIssuingIt(t *testing.T) {
	for _, tc := range []struct{ language, taxMode string }{{"en", "standard"}, {"de", "small_business"}} {
		t.Run(tc.language, func(t *testing.T) {
			ta := newTestApp(t, devConfig+strings.Replace(businessConfig, `"standard"`, strconv.Quote(tc.taxMode), 1))
			c := ta.client()
			form := customerForm("acme", "85")
			form.Set("invoice_language", tc.language)
			c.createCustomer(form)
			cust := c.customerPath("acme")
			c.createProject(cust, projectForm("website", ""))
			c.createEntry(entryForm(strings.TrimPrefix(c.projectPath(cust, "website"), "/projects/"), "2026-09-02T08:00", "2026-09-02T09:00", "design"))
			draft := c.createDraft(draftForm(cust, "2026-09-01", "2026-09-30"))
			c.addLine(draft, lineForm("Workshop", "1.5", "85"))

			res := c.get(draft + "/preview")

			res.wantStatus(t, http.StatusOK)
			if ct := res.Header.Get("Content-Type"); ct != "application/pdf" {
				t.Fatalf("Content-Type = %q", ct)
			}
			if !strings.HasPrefix(res.body, "%PDF-") {
				t.Fatalf("body is not a PDF: %.40q", res.body)
			}
			c.get("/invoices?status=draft").wantBody(t, `href="`+draft+`"`)
		})
	}
}
