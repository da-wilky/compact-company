package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

func snippetForm(name, language, heading, body string) url.Values {
	return url.Values{"name": {name}, "language": {language}, "heading": {heading}, "body": {body}}
}

func (c *client) createSnippet(form url.Values) {
	c.t.Helper()
	wantRedirect(c.t, c.postForm("/snippets", form), "/snippets")
}

func TestSnippetsAreListedByLanguageAndName(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.createSnippet(snippetForm("Terms", "en", "Terms", "Net 14 days."))
	c.createSnippet(snippetForm("Zahlung", "de", "Zahlungsbedingungen", "14 Tage netto."))
	c.createSnippet(snippetForm("Schedule", "en", "", "Two weeks."))

	res := c.get("/snippets")
	res.wantStatus(t, http.StatusOK)
	wantInOrder(t, res.text(), "Zahlung", "Deutsch", "Zahlungsbedingungen", "Schedule", "English", "Terms", "English", "Terms")
}

var snippetLink = regexp.MustCompile(`href="(/snippets/\d+)">([^<]*)<`)

// snippetPath returns the path of the snippet the Snippets page lists as name.
func (c *client) snippetPath(name string) string {
	c.t.Helper()
	for _, m := range snippetLink.FindAllStringSubmatch(c.get("/snippets").body, -1) {
		if m[2] == name {
			return m[1]
		}
	}
	c.t.Fatalf("no snippet %q", name)
	return ""
}

func TestSnippetsCanBeEditedAndDeleted(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	body := "Net *14* days.\n\n- no discount\n  "
	c.createSnippet(snippetForm("Terms", "en", "Terms", body))
	c.createSnippet(snippetForm("Schedule", "en", "", "Two weeks."))

	edit := c.get(c.snippetPath("Terms"))
	edit.wantBody(t, `name="name" value="Terms"`)
	edit.wantBody(t, `<option value="en" selected>`)
	edit.wantBody(t, ">"+body+"</textarea>")
	wantRedirect(t, c.postForm(c.snippetPath("Terms"), snippetForm("Zahlung", "de", "Zahlung", "14 Tage netto.")), "/snippets")

	res := c.get("/snippets")
	wantInOrder(t, res.text(), "Zahlung", "Deutsch", "Schedule", "English")
	res.wantNoBody(t, "Terms")

	wantRedirect(t, c.postForm(c.snippetPath("Zahlung")+"/delete", nil), "/snippets")

	res = c.get("/snippets")
	res.wantNoBody(t, "Zahlung")
	res.wantBody(t, "Schedule")
}

func TestSnippetsAreValidated(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for name, tc := range map[string]struct {
		form    url.Values
		message string
	}{
		"no name":     {snippetForm(" ", "en", "Terms", "Net 14 days."), "Name is required."},
		"no language": {snippetForm("Terms", "fr", "Terms", "Net 14 days."), "Choose a language."},
		"no body":     {snippetForm("Terms", "en", "Terms", " \n "), "Text is required."},
	} {
		t.Run(name, func(t *testing.T) {
			res := c.postForm("/snippets", tc.form)

			res.wantStatus(t, http.StatusUnprocessableEntity)
			res.wantBody(t, tc.message)
		})
	}
	c.get("/snippets").wantBody(t, "No snippets yet.")
}

func TestSnippetThatFailsToCompileIsNotSaved(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	body := "Payment within 14 days.\r\n\r\nEach reminder costs 5$."

	res := c.postForm("/snippets", snippetForm("Terms", "en", "Terms", body))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Paragraph 2 doesn&#39;t compile: unclosed delimiter")
	res.wantBody(t, `Write 5\$`)
	res.wantBody(t, ">"+body+"</textarea>")
	c.get("/snippets").wantBody(t, "No snippets yet.")

	c.createSnippet(snippetForm("Terms", "en", "Terms", "Payment within 14 days."))
	res = c.postForm(c.snippetPath("Terms"), snippetForm("Terms", "en", "Terms", body))

	res.wantStatus(t, http.StatusUnprocessableEntity)
	c.get(c.snippetPath("Terms")).wantBody(t, ">Payment within 14 days.</textarea>")
}

func (c *client) insertSnippet(quote, name string) *response {
	c.t.Helper()
	return c.postForm(quote+"/snippets", url.Values{"snippet": {strings.TrimPrefix(c.snippetPath(name), "/snippets/")}})
}

func TestSnippetInsertedIntoAQuoteIsACopyAtTheEnd(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	c.addSection(quote, sectionForm("Scope", "Three pages."))
	c.createSnippet(snippetForm("Payment terms", "en", "Terms", "Net *14* days."))

	wantRedirect(t, c.insertSnippet(quote, "Payment terms"), quote)

	wantInOrder(t, c.get(quote).text(), "Scope", "Three pages.", "Terms", "Net *14* days.")
	wantRedirect(t, c.postForm(c.snippetPath("Payment terms"), snippetForm("Payment terms", "en", "Payment", "Net 30 days.")), "/snippets")
	wantInOrder(t, c.get(quote).text(), "Scope", "Terms", "Net *14* days.")
	wantRedirect(t, c.postForm(c.snippetPath("Payment terms")+"/delete", nil), "/snippets")
	wantInOrder(t, c.get(quote).text(), "Scope", "Terms", "Net *14* days.")
}

func TestSnippetPickerOffersOnlyTheQuotesLanguage(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	c.createSnippet(snippetForm("Payment terms", "en", "Terms", "Net 14 days."))
	c.createSnippet(snippetForm("Zahlung", "de", "Zahlungsbedingungen", "14 Tage netto."))

	res := c.get(quote)
	res.wantBody(t, ">Payment terms</option>")
	res.wantNoBody(t, "Zahlung")

	res = c.insertSnippet(quote, "Zahlung")

	res.wantStatus(t, http.StatusUnprocessableEntity)
	res.wantBody(t, "Choose a snippet in the quote’s language.")
	c.get(quote).wantBody(t, "No sections yet.")
	c.postForm(quote+"/snippets", url.Values{"snippet": {"99"}}).wantStatus(t, http.StatusUnprocessableEntity)
}

func TestQuoteWithoutSnippetsInItsLanguageLinksToSnippets(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	c.createSnippet(snippetForm("Zahlung", "de", "Zahlungsbedingungen", "14 Tage netto."))

	res := c.get(quote)

	res.wantNoBody(t, `name="snippet"`)
	res.wantBody(t, `href="/snippets"`)
}

func TestUnknownSnippetIsNotFound(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	c.get("/snippets/99").wantStatus(t, http.StatusNotFound)
	c.postForm("/snippets/99", snippetForm("Terms", "en", "", "Text.")).wantStatus(t, http.StatusNotFound)
	c.postForm("/snippets/99/delete", nil).wantStatus(t, http.StatusNotFound)
}
