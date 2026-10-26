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
	Lang   string
	User   *User
	Timers *timerBar
	Data   any
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
		t, err := template.New(name).Funcs(template.FuncMap{"t": fmt.Sprintf, "datetime": time.Time.String, "clock": time.Time.String, "date": time.Time.String, "day": strings.Clone, "duration": formatDuration, "hours": formatHours}).
			ParseFS(templates, "templates/layout.html", "templates/partials/*.html", f)
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

var dateLayouts = map[string]string{"en": "2006-01-02", "de": "02.01.2006"}

// viewerLocation is the zone all times are shown and attributed to days in:
// the user's timezone, or the configured default before login.
func (a *App) viewerLocation(r *http.Request) (*time.Location, error) {
	if u := currentUser(r); u != nil {
		return time.LoadLocation(u.Timezone)
	}
	d, err := a.defaults()
	if err != nil {
		return nil, err
	}
	return time.LoadLocation(d.Timezone)
}

// render writes the page in the viewer's language, or the configured default before login.
func (a *App) render(w http.ResponseWriter, r *http.Request, status int, page string, data any) {
	a.execute(w, r, status, page, "layout", view{Data: data})
}

func (a *App) execute(w http.ResponseWriter, r *http.Request, status int, page, name string, v view) {
	if err := a.write(w, r, status, page, name, v); err != nil {
		a.serverError(w, r, err)
	}
}

// write runs template name from page's set; v.Timers, if unset, is loaded for a logged-in user.
func (a *App) write(w http.ResponseWriter, r *http.Request, status int, page, name string, v view) error {
	u := currentUser(r)
	var lang string
	if u != nil {
		lang = u.Language
	} else {
		d, err := a.defaults()
		if err != nil {
			return err
		}
		lang = d.Language
	}
	loc, err := a.viewerLocation(r)
	if err != nil {
		return err
	}
	if u != nil && v.Timers == nil {
		if v.Timers, err = a.loadTimerBar(u.ID); err != nil {
			return err
		}
	}
	p := message.NewPrinter(language.Make(lang), message.Catalog(a.catalog))
	t, err := a.pages[page].Clone()
	if err != nil {
		return err
	}
	t.Funcs(template.FuncMap{
		"t":        p.Sprintf,
		"datetime": func(ts time.Time) string { return ts.In(loc).Format(datetimeLayouts[lang]) },
		"date":     func(ts time.Time) string { return ts.In(loc).Format(dateLayouts[lang]) },
		"clock":    func(ts time.Time) string { return ts.In(loc).Format("15:04") },
		// day formats a stored calendar date, which is already in the customer's timezone.
		"day": func(iso string) (string, error) {
			d, err := time.Parse(time.DateOnly, iso)
			return d.Format(dateLayouts[lang]), err
		},
	})
	var buf bytes.Buffer
	v.Lang, v.User = lang, u
	if err := t.ExecuteTemplate(&buf, name, v); err != nil {
		return err
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(status)
	_, _ = buf.WriteTo(w)
	return nil
}
