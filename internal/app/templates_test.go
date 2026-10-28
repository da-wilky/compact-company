package app

import (
	"io/fs"
	"path"
	"regexp"
	"strings"
	"testing"
)

// Page anatomy from docs/agents/codebase.md, checked on the template sources.

var mainNavPages = map[string]bool{"home": true, "week": true, "entries": true, "invoices": true, "customers": true, "settings": true, "profile": true}

// Pages outside the app shell's navigation: login, access refusal, and the error page with its own way home.
var unnavigatedPages = map[string]bool{"login": true, "not_allowed": true, "error": true}

func templateSources(t *testing.T) map[string]string {
	t.Helper()
	files, err := fs.Glob(templates, "templates/*.html")
	if err != nil {
		t.Fatal(err)
	}
	partials, err := fs.Glob(templates, "templates/partials/*.html")
	if err != nil {
		t.Fatal(err)
	}
	sources := map[string]string{}
	for _, f := range append(files, partials...) {
		b, err := fs.ReadFile(templates, f)
		if err != nil {
			t.Fatal(err)
		}
		sources[f] = string(b)
	}
	return sources
}

func TestEveryPageOffTheMainNavHasOneCrumb(t *testing.T) {
	for f, src := range templateSources(t) {
		name := strings.TrimSuffix(path.Base(f), ".html")
		if path.Dir(f) != "templates" || name == "layout" || unnavigatedPages[name] {
			continue
		}
		want := 1
		if mainNavPages[name] {
			want = 0
		}
		if got := strings.Count(src, `<a class="crumb"`); got != want {
			t.Errorf("%s: %d crumbs, want %d", f, got, want)
		}
	}
}

var (
	divTag   = regexp.MustCompile(`<div[\s>]|</div>`)
	pagerNav = regexp.MustCompile(`(?s)<nav class="pager".*?</nav>`)
)

func TestPageHeadHoldsNoPillOutsideThePager(t *testing.T) {
	for f, src := range templateSources(t) {
		for rest := src; ; {
			i := strings.Index(rest, `<div class="page-head">`)
			if i < 0 {
				break
			}
			rest = rest[i:]
			depth, end := 0, len(rest)
			for _, m := range divTag.FindAllStringIndex(rest, -1) {
				if rest[m[0]+1] == '/' {
					depth--
				} else {
					depth++
				}
				if depth == 0 {
					end = m[1]
					break
				}
			}
			if strings.Contains(pagerNav.ReplaceAllString(rest[:end], ""), `class="pill`) {
				t.Errorf("%s: .page-head holds a .pill outside its .pager", f)
			}
			rest = rest[end:]
		}
	}
}

var (
	errorClass = regexp.MustCompile(`class="([^"]* )?error( [^"]*)?"`)
	errorsList = regexp.MustCompile(`<ul class="errors"[^>]*>`)
)

func TestErrorsRenderOnlyAsAlertLists(t *testing.T) {
	for f, src := range templateSources(t) {
		if errorClass.MatchString(src) {
			t.Errorf("%s: error shown outside ul.errors", f)
		}
		for _, ul := range errorsList.FindAllString(src, -1) {
			if !strings.Contains(ul, `role="alert"`) {
				t.Errorf("%s: %s lacks role=\"alert\"", f, ul)
			}
		}
	}
}

func TestFormsUseTheFormCard(t *testing.T) {
	for f, src := range templateSources(t) {
		if strings.Contains(src, `class="settings"`) {
			t.Errorf("%s: form uses .settings instead of .form", f)
		}
	}
}
