package app

import (
	"bytes"
	"embed"
	"fmt"
	"html/template"
	"io/fs"
	"net/http"
	"path"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
	"golang.org/x/text/language"
	"golang.org/x/text/message"
	"golang.org/x/text/message/catalog"
)

//go:embed templates
var templates embed.FS

//go:embed messages
var messages embed.FS

var languages = []language.Tag{language.English, language.German}

func supportedLanguage(lang string) bool {
	for _, tag := range languages {
		if tag.String() == lang {
			return true
		}
	}
	return false
}

type view struct {
	Lang string
	User *User
	Data any
}

// parsePages builds one template set per page: layout.html plus templates/<page>.html.
func parsePages() (map[string]*template.Template, error) {
	files, err := fs.Glob(templates, "templates/*.html")
	if err != nil {
		return nil, err
	}
	pages := map[string]*template.Template{}
	for _, f := range files {
		name := strings.TrimSuffix(path.Base(f), ".html")
		if name == "layout" {
			continue
		}
		// "t" is rebound per request to the viewer's language in render.
		t, err := template.New(name).Funcs(template.FuncMap{"t": fmt.Sprintf, "datetime": time.Time.String}).
			ParseFS(templates, "templates/layout.html", f)
		if err != nil {
			return nil, err
		}
		pages[name] = t
	}
	return pages, nil
}

// loadCatalog merges messages/*.toml; every key must be translated into every language exactly once.
func loadCatalog() (catalog.Catalog, error) {
	b := catalog.NewBuilder(catalog.Fallback(language.English))
	files, err := fs.Glob(messages, "messages/*.toml")
	if err != nil {
		return nil, err
	}
	seen := map[string]string{}
	for _, f := range files {
		var entries map[string]map[string]string
		if _, err := toml.DecodeFS(messages, f, &entries); err != nil {
			return nil, err
		}
		for key, translations := range entries {
			if other, dup := seen[key]; dup {
				return nil, fmt.Errorf("message %q defined in %s and %s", key, other, f)
			}
			seen[key] = f
			if len(translations) != len(languages) {
				return nil, fmt.Errorf("%s: message %q must have exactly the languages %v", f, key, languages)
			}
			for _, tag := range languages {
				s, ok := translations[tag.String()]
				if !ok {
					return nil, fmt.Errorf("%s: message %q lacks %s", f, key, tag)
				}
				if err := b.SetString(tag, key, s); err != nil {
					return nil, err
				}
			}
		}
	}
	return b, nil
}

var datetimeLayouts = map[string]string{"en": "2006-01-02 15:04", "de": "02.01.2006 15:04"}

// viewerLocation is the zone all times are shown and attributed to days in:
// the user's timezone, or the configured default before login.
func (a *App) viewerLocation(r *http.Request) (*time.Location, error) {
	name := a.cfg.Defaults.Timezone
	if u := currentUser(r); u != nil {
		name = u.Timezone
	}
	return time.LoadLocation(name)
}

// render writes the page in the viewer's language, or the configured default before login.
func (a *App) render(w http.ResponseWriter, r *http.Request, status int, page string, data any) {
	u := currentUser(r)
	lang := a.cfg.Defaults.Language
	if u != nil {
		lang = u.Language
	}
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	p := message.NewPrinter(language.Make(lang), message.Catalog(a.catalog))
	t, err := a.pages[page].Clone()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	t.Funcs(template.FuncMap{
		"t":        p.Sprintf,
		"datetime": func(ts time.Time) string { return ts.In(loc).Format(datetimeLayouts[lang]) },
	})
	var buf bytes.Buffer
	if err := t.ExecuteTemplate(&buf, "layout", view{Lang: lang, User: u, Data: data}); err != nil {
		a.serverError(w, r, err)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(status)
	_, _ = buf.WriteTo(w)
}
