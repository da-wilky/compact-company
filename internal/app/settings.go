package app

import (
	"net/http"
	"strconv"

	"golang.org/x/text/currency"

	"github.com/da-wilky/compact-company/internal/config"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /settings", a.authed(a.settingsPage))
		mux.HandleFunc("POST /settings/defaults", a.authed(a.defaultsSave))
	})
}

type settingsPage struct {
	Defaults  config.Defaults
	Managed   config.ManagedDefaults
	Languages []languageOption
	Error     string
	Business  businessPage
}

func (a *App) storedDefaults() (config.Defaults, error) {
	var d config.Defaults
	err := a.db.QueryRow(`SELECT language, timezone, currency, parallel_entries FROM defaults`).
		Scan(&d.Language, &d.Timezone, &d.Currency, &d.ParallelEntries)
	return d, err
}

// defaults returns the stored defaults with the keys config sets laid over them.
func (a *App) defaults() (config.Defaults, error) {
	d, err := a.storedDefaults()
	if err != nil {
		return d, err
	}
	return a.overlayManaged(d, a.cfg.Defaults), nil
}

func (a *App) overlayManaged(d, src config.Defaults) config.Defaults {
	set := a.cfg.DefaultsManaged
	if set.Language {
		d.Language = src.Language
	}
	if set.Timezone {
		d.Timezone = src.Timezone
	}
	if set.Currency {
		d.Currency = src.Currency
	}
	if set.ParallelEntries {
		d.ParallelEntries = src.ParallelEntries
	}
	return d
}

func (a *App) loadSettings() (settingsPage, error) {
	d, err := a.defaults()
	if err != nil {
		return settingsPage{}, err
	}
	b, err := a.loadBusiness()
	if err != nil {
		return settingsPage{}, err
	}
	return settingsPage{Defaults: d, Managed: a.cfg.DefaultsManaged, Languages: languageOptions(),
		Business: a.businessSection(businessPage{Business: b, VATRate: strconv.Itoa(b.VATRate)})}, nil
}

func (a *App) settingsPage(w http.ResponseWriter, r *http.Request) {
	p, err := a.loadSettings()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "settings", p)
}

// defaultsSave keeps the stored value of every key config sets, so posting one changes nothing.
func (a *App) defaultsSave(w http.ResponseWriter, r *http.Request) {
	stored, err := a.storedDefaults()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	d := a.overlayManaged(config.Defaults{
		Language:        r.PostFormValue("language"),
		Timezone:        r.PostFormValue("timezone"),
		Currency:        r.PostFormValue("currency"),
		ParallelEntries: r.PostFormValue("parallel_entries") != "",
	}, stored)
	cur, curErr := currency.ParseISO(d.Currency)
	var errKey string
	switch {
	case !supportedLanguage(d.Language):
		errKey = "settings.error.language"
	case !validTimezone(d.Timezone):
		errKey = "settings.error.timezone"
	case curErr != nil:
		errKey = "settings.error.currency"
	}
	if errKey != "" {
		p, err := a.loadSettings()
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		p.Defaults, p.Error = a.overlayManaged(d, a.cfg.Defaults), errKey
		a.render(w, r, http.StatusUnprocessableEntity, "settings", p)
		return
	}
	if _, err := a.db.Exec(`UPDATE defaults SET language = ?, timezone = ?, currency = ?, parallel_entries = ?`,
		d.Language, d.Timezone, cur.String(), d.ParallelEntries); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/settings", http.StatusSeeOther)
}
