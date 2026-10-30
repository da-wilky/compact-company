package app_test

import (
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"testing"
)

var listItem = regexp.MustCompile(`(?s)<li[ >].*?</li>`)

// lint posts body to the lint the section form runs while typing and returns each finding as the user reads it.
func lint(t *testing.T, c *client, body string) []string {
	t.Helper()
	res := c.postForm("/typst/lint", url.Values{"body": {body}})
	res.wantStatus(t, http.StatusOK)
	var findings []string
	for _, li := range listItem.FindAllString(res.body, -1) {
		findings = append(findings, textOf(li))
	}
	return findings
}

func TestLintFlagsTypstPitfallsWithTheirEscapedForm(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for name, tc := range map[string]struct {
		body string
		want []string
	}{
		"mail address":           {"Write to info@swilk.eu.", []string{`“info@swilk.eu” reads as a reference. Write info\@swilk.eu`}},
		"handle":                 {"Ask @team first.", []string{`\@team`}},
		"lone dollar":            {"It costs 5$ only.", []string{`5\$`}},
		"dollar before a number": {"It costs $5.", []string{`“$5” starts a formula`}},
		"unclosed bold":          {"This is *really important.", []string{`\*really`}},
		"bold across paragraphs": {"*Phase one\n\nends here*", []string{`\*Phase`, `here\*`}},
		"bold unclosed in block": {"#strong[a *b] c", []string{`\*b]`}},
		"multiplication":         {"5 * 3 = 15", []string{`\*`}},
		"unclosed italic":        {"_draft version", []string{`\_draft`}},
		"trailing underscore":    {"file_ name", []string{`file\_`}},
		"hashtag":                {"See #roadmap for details.", []string{`\#roadmap`}},
		"hash after a word":      {"We write C# code.", []string{`C\#`}},
		"several":                {"info@swilk.eu and 5$", []string{`info\@swilk.eu`, `5\$`}},
	} {
		t.Run(name, func(t *testing.T) {
			got := lint(t, c, tc.body)
			if len(got) != len(tc.want) {
				t.Fatalf("findings = %q, want %d of them: %q", got, len(tc.want), tc.want)
			}
			for i, w := range tc.want {
				if !strings.Contains(got[i], w) {
					t.Errorf("finding %d = %q, want it to offer %q", i, got[i], w)
				}
			}
		})
	}
}

func TestSavedSectionNamesTheParagraphThatFailsToCompile(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	quote := c.acmeQuote()
	c.addSection(quote, sectionForm("Scope", "All pages.\r\n\r\nIt costs 5$ only.\r\n\r\nThanks."))
	c.addSection(quote, sectionForm("Terms", "Net *14* days."))

	res := c.get(quote)

	wantInOrder(t, res.text(), "Scope", "Paragraph 2 doesn't compile: unclosed delimiter", "It costs 5$ only.", "Terms", "Net *14* days.")
	if n := strings.Count(res.text(), "doesn't compile"); n != 1 {
		t.Errorf("%d failures shown, want only the one in Scope", n)
	}
	c.get(c.quotePart(quote, "Scope")).wantBody(t, "Paragraph 2 doesn&#39;t compile: unclosed delimiter")
	c.get(c.quotePart(quote, "Terms")).wantNoBody(t, "compile")
}

// markup is the behaviour: the button's value is what clicking it sends.
var escapeButton = regexp.MustCompile(`name="escape" value="(\d+)"`)

func TestLintFindingEscapesInOneClick(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	body := "Mail info@swilk.eu, it costs 5$."
	res := c.postForm("/typst/lint", url.Values{"body": {body}})
	buttons := escapeButton.FindAllStringSubmatch(res.body, -1)
	if len(buttons) != 2 {
		t.Fatalf("want two escape buttons:\n%s", res.body)
	}

	res = c.postForm("/typst/lint", url.Values{"body": {body}, "escape": {buttons[1][1]}})

	res.wantStatus(t, http.StatusOK)
	res.wantBody(t, ">Mail info@swilk.eu, it costs 5\\$.</textarea>")
	res.wantBody(t, `Write info\@swilk.eu<`)
	res.wantNoBody(t, `Write 5\$`)
}

func TestLintLeavesValidMarkupAlone(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()
	for _, body := range []string{
		"Plain prose, with commas; and: colons.",
		"*bold* and _italic_ and *bold _both_*",
		"*bold\nacross lines*",
		"snake_case and a*b and 2*3*4",
		"#link(\"https://swilk.eu\")[our site]",
		"#link(\"mailto:info@swilk.eu\")[mail us]",
		"#strong[*nested*] text",
		"#text(size: 2*3pt)[small] print",
		"#set text(lang: \"de\")\nHallo",
		"#pagebreak()",
		"#sym.arrow.r and #none",
		"Issue #5 is fixed.",
		"$x^2$ and $a_1$",
		"A price of 5 \\$ and info\\@swilk.eu and \\#tag and \\* and \\_",
		"`raw *star* and info@swilk.eu`",
		"```\ncode $ here\n```",
		"https://example.com/@me/a_b*c",
		"A @ B and a@ and an email@ ",
		"// a comment with * and $\nText",
		"/* block * $ */ Text",
		"- item one\n- item two\n+ numbered",
		"#table(columns: 2, [A], [B])",
	} {
		t.Run(body, func(t *testing.T) {
			if got := lint(t, c, body); len(got) != 0 {
				t.Errorf("findings = %q, want none", got)
			}
		})
	}
}
