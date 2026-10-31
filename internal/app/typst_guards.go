package app

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /typst/lint", a.authed(a.typstLint))
	})
}

// typstFinding is a pitfall of ordinary prose in Typst markup (ADR 0007). Pos is the
// byte offset of the character a backslash escapes; Text is the word around it as
// typed, Escaped the same word with that backslash.
type typstFinding struct {
	Kind          string
	Pos           int
	Text, Escaped string
}

// lintTypst finds the prose pitfalls of ADR 0007 in body without running Typst:
// an @ starting a reference, a lone $, an unclosed * or _, and a # not starting code.
func lintTypst(body string) []typstFinding {
	l := &typstLinter{src: body}
	l.markup(false)
	slices.SortFunc(l.findings, func(a, b typstFinding) int { return a.Pos - b.Pos })
	return l.findings
}

// escapeTypst escapes the finding at pos, if body has one there.
func escapeTypst(body string, pos int) string {
	if slices.ContainsFunc(lintTypst(body), func(f typstFinding) bool { return f.Pos == pos }) {
		return body[:pos] + `\` + body[pos:]
	}
	return body
}

// typstLinter follows Typst's markup lexer just far enough to know which
// characters are markup syntax and which sit in code, strings, raw text or math.
type typstLinter struct {
	src      string
	i        int
	open     []int // unclosed * and _
	findings []typstFinding
}

var (
	typstStatements = []string{"let", "set", "show", "if", "for", "while", "import", "include", "context", "return", "break", "continue"}
	typstValues     = []string{"none", "auto", "true", "false"}
)

func (l *typstLinter) flag(kind string, pos int) {
	start := 0
	if n := strings.LastIndexFunc(l.src[:pos], unicode.IsSpace); n >= 0 {
		_, size := utf8.DecodeRuneInString(l.src[n:])
		start = n + size
	}
	end := len(l.src)
	if n := strings.IndexFunc(l.src[pos:], unicode.IsSpace); n >= 0 {
		end = pos + n
	}
	for end > pos+1 && strings.IndexByte(".,;:!?", l.src[end-1]) >= 0 {
		end--
	}
	l.findings = append(l.findings, typstFinding{Kind: kind, Pos: pos, Text: l.src[start:end], Escaped: l.src[start:pos] + `\` + l.src[pos:end]})
}

func (l *typstLinter) peek(off int) byte {
	if l.i+off < len(l.src) {
		return l.src[l.i+off]
	}
	return 0
}

// closeDelimiters flags the * and _ opened since base: a paragraph or content block ends them.
func (l *typstLinter) closeDelimiters(base int) {
	for _, pos := range l.open[base:] {
		l.flag(map[byte]string{'*': "strong", '_': "emph"}[l.src[pos]], pos)
	}
	l.open = l.open[:base]
}

// markup scans to the end of the body or, in a content block, past its closing ].
func (l *typstLinter) markup(inBlock bool) {
	base := len(l.open)
	defer l.closeDelimiters(base)
	for l.i < len(l.src) {
		c := l.src[l.i]
		switch {
		case c == '\\':
			l.i += 2
		case c == ']' && inBlock:
			l.i++
			return
		case c == '[' && inBlock:
			// A nested pair of brackets is plain text inside a content block.
			l.i++
			l.markup(true)
		case c == '`':
			l.raw()
		case c == '$':
			l.math(inBlock)
		case c == '*' || c == '_':
			l.delimiter(base)
		case c == '\n':
			l.i++
			j := l.i
			for j < len(l.src) && strings.IndexByte(" \t\r", l.src[j]) >= 0 {
				j++
			}
			if j < len(l.src) && l.src[j] == '\n' {
				l.closeDelimiters(base)
				l.i = j
			}
		case c == '@':
			l.i++
			if r, _ := utf8.DecodeRuneInString(l.src[l.i:]); unicode.IsLetter(r) || unicode.IsDigit(r) || r == '_' {
				l.flag("at", l.i-1)
			}
		case c == '#':
			l.hash()
		case c == '/' && l.peek(1) == '/':
			l.skipLine()
		case c == '/' && l.peek(1) == '*':
			l.skipPast("*/")
		case c == '<':
			l.i++
			if m := typstLabel.FindStringIndex(l.src[l.i:]); m != nil {
				l.i += m[1]
			}
		case strings.HasPrefix(l.src[l.i:], "http://") || strings.HasPrefix(l.src[l.i:], "https://"):
			for l.i < len(l.src) && !unicode.IsSpace(rune(l.src[l.i])) && l.src[l.i] != ']' {
				l.i++
			}
		default:
			l.i++
		}
	}
}

var typstLabel = regexp.MustCompile(`^[\pL\pN_\-.:]+>`)

// delimiter handles * and _, which Typst reads as text inside a word.
func (l *typstLinter) delimiter(base int) {
	prev, _ := utf8.DecodeLastRuneInString(l.src[:l.i])
	next, _ := utf8.DecodeRuneInString(l.src[l.i+1:])
	wordy := func(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }
	switch {
	case wordy(prev) && wordy(next):
	case len(l.open) > base && l.src[l.open[len(l.open)-1]] == l.src[l.i]:
		l.open = l.open[:len(l.open)-1]
	default:
		l.open = append(l.open, l.i)
	}
	l.i++
}

func (l *typstLinter) raw() {
	n := 0
	for l.peek(n) == '`' {
		n++
	}
	l.i += n
	if n == 2 {
		return
	}
	l.skipPast(strings.Repeat("`", n))
}

// math skips a formula, flagging its $ when nothing closes it.
func (l *typstLinter) math(inBlock bool) {
	start := l.i
	for l.i++; l.i < len(l.src); l.i++ {
		switch l.src[l.i] {
		case '\\':
			l.i++
		case '"':
			l.str()
			l.i--
		case '$':
			l.i++
			return
		case ']':
			if inBlock {
				l.i = start + 1
				l.flag("dollar", start)
				return
			}
		}
	}
	l.i = start + 1
	l.flag("dollar", start)
}

// hash reads what follows a #: code, or a pitfall when no expression starts there.
func (l *typstLinter) hash() {
	pos := l.i
	l.i++
	c := l.peek(0)
	switch {
	case c >= '0' && c <= '9':
	case c == '(' || c == '{':
		l.i++
		l.code(map[byte]byte{'(': ')', '{': '}'}[c])
	case c == '[':
		l.i++
		l.markup(true)
	case c == '"':
		l.str()
	case c == '_' || unicode.IsLetter(rune(c)) || c >= utf8.RuneSelf:
		name := l.ident()
		if slices.Contains(typstStatements, name) {
			l.codeLine()
			return
		}
		if c := l.peek(0); c != '(' && c != '[' && c != '.' && !slices.Contains(typstValues, name) {
			l.flag("hash", pos)
			return
		}
		l.call()
	default:
		l.flag("hash", pos)
	}
}

func (l *typstLinter) ident() string {
	start := l.i
	for l.i < len(l.src) {
		r, n := utf8.DecodeRuneInString(l.src[l.i:])
		if !unicode.IsLetter(r) && !unicode.IsDigit(r) && r != '_' && r != '-' {
			break
		}
		l.i += n
	}
	return l.src[start:l.i]
}

// call skips field access and arguments, and lints the content blocks passed as markup.
func (l *typstLinter) call() {
	for {
		switch l.peek(0) {
		case '.':
			l.i++
			l.ident()
		case '(':
			l.i++
			l.code(')')
		case '[':
			l.i++
			l.markup(true)
		default:
			return
		}
	}
}

// code skips code up to and past closing, linting content blocks inside it as markup.
func (l *typstLinter) code(closing byte) {
	for l.i < len(l.src) {
		c := l.src[l.i]
		l.i++
		switch c {
		case closing:
			return
		case '"':
			l.i--
			l.str()
		case '(':
			l.code(')')
		case '{':
			l.code('}')
		case '[':
			l.markup(true)
		case '/':
			if l.peek(0) == '/' {
				l.skipLine()
			}
		}
	}
}

// codeLine skips a #let, #set, #show and similar statement, which ends with its line.
func (l *typstLinter) codeLine() {
	for l.i < len(l.src) && l.src[l.i] != '\n' {
		c := l.src[l.i]
		l.i++
		switch c {
		case '"':
			l.i--
			l.str()
		case '(':
			l.code(')')
		case '{':
			l.code('}')
		case '[':
			l.markup(true)
		}
	}
}

// str skips a string literal starting at the current ".
func (l *typstLinter) str() {
	for l.i++; l.i < len(l.src); l.i++ {
		switch l.src[l.i] {
		case '\\':
			l.i++
		case '"':
			l.i++
			return
		}
	}
}

func (l *typstLinter) skipLine() {
	if n := strings.IndexByte(l.src[l.i:], '\n'); n >= 0 {
		l.i += n
		return
	}
	l.i = len(l.src)
}

func (l *typstLinter) skipPast(end string) {
	if n := strings.Index(l.src[l.i:], end); n >= 0 {
		l.i += n + len(end)
		return
	}
	l.i = len(l.src)
}

// typstFailure is a body that doesn't compile: the first blank-line separated
// paragraph (counted from 1) that fails on its own, or 0 when none does alone.
type typstFailure struct {
	Paragraph int
	Text      string
	Message   string
}

// typstCheckEntry evaluates a body as every quote.typ must (ADR 0007).
const typstCheckEntry = "#eval(sys.inputs.body, mode: \"markup\")\n"

// checkTypst compiles body alone in a minimal document; a nil failure means it compiles.
// The error is for typst not running at all.
func checkTypst(ctx context.Context, body string) (*typstFailure, error) {
	// A directory of its own as root keeps #read and #include away from every other file.
	dir, err := os.MkdirTemp("", "compact-company-check-")
	if err != nil {
		return nil, err
	}
	defer func() { _ = os.RemoveAll(dir) }()
	entry := filepath.Join(dir, "check.typ")
	if err := os.WriteFile(entry, []byte(typstCheckEntry), 0o600); err != nil {
		return nil, err
	}
	compile := func(src string) (string, error) {
		cmd := exec.CommandContext(ctx, "typst", "compile", "--root", dir, "--ignore-system-fonts", "--format", "pdf", "--input", "body="+src, entry, "-")
		var stderr bytes.Buffer
		cmd.Stderr = &stderr
		err := cmd.Run()
		var exit *exec.ExitError
		if errors.As(err, &exit) && ctx.Err() == nil {
			return typstMessage(stderr.String(), exit), nil
		}
		return "", err
	}
	msg, err := compile(body)
	if msg == "" {
		return nil, err
	}
	for i, p := range paragraphs(body) {
		m, err := compile(p)
		if err != nil {
			return nil, err
		}
		if m != "" {
			return &typstFailure{Paragraph: i + 1, Text: p, Message: m}, nil
		}
	}
	return &typstFailure{Message: msg}, nil
}

// typstMessage is the first error typst reports, without the source excerpt that points into the check document.
func typstMessage(stderr string, exit *exec.ExitError) string {
	for line := range strings.Lines(stderr) {
		if msg, ok := strings.CutPrefix(line, "error: "); ok {
			return strings.TrimSpace(msg)
		}
	}
	if msg := strings.TrimSpace(stderr); msg != "" {
		return msg
	}
	return exit.Error()
}

var paragraphBreak = regexp.MustCompile(`\n[ \t\r]*\n`)

func paragraphs(body string) []string {
	var ps []string
	for _, p := range paragraphBreak.Split(body, -1) {
		if p = strings.TrimSpace(p); p != "" {
			ps = append(ps, p)
		}
	}
	return ps
}

// typstBody is the Typst field of a form with its lint findings (partial "typst-body").
type typstBody struct {
	Body     string
	Findings []typstFinding
}

func newTypstBody(body string) typstBody {
	return typstBody{Body: body, Findings: lintTypst(body)}
}

// typstLint answers the field while typing; escape names the Pos of a finding to escape.
func (a *App) typstLint(w http.ResponseWriter, r *http.Request) {
	body := r.PostFormValue("body")
	if pos, err := strconv.Atoi(r.PostFormValue("escape")); err == nil {
		body = escapeTypst(body, pos)
	}
	a.execute(w, r, http.StatusOK, "quote_section", "typst-lint", view{Data: newTypstBody(body)})
}
