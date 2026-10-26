package app

import (
	"net/http"
	"time"

	"github.com/da-wilky/compact-company/internal/config"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /profile", a.authed(a.profilePage))
		mux.HandleFunc("POST /profile", a.authed(a.profileSave))
	})
}

type profileForm struct {
	Language        string
	Timezone        string
	ParallelEntries bool
	Error           string
	Now             time.Time
	Languages       []languageOption
	Default         config.Defaults
}

func (a *App) profilePage(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	d, err := a.defaults()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "profile", profileForm{Language: u.OwnLanguage, Timezone: u.OwnTimezone, ParallelEntries: u.ParallelEntries,
		Now: a.now(), Languages: languageOptions(), Default: d})
}

func (a *App) profileSave(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	d, err := a.defaults()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	f := profileForm{
		Language:        r.PostFormValue("language"),
		Timezone:        r.PostFormValue("timezone"),
		ParallelEntries: r.PostFormValue("parallel_entries") != "",
		Now:             a.now(),
		Languages:       languageOptions(),
		Default:         d,
	}
	switch {
	case f.Language != "" && !supportedLanguage(f.Language):
		f.Error = "settings.error.language"
	case f.Timezone != "" && !validTimezone(f.Timezone):
		f.Error = "settings.error.timezone"
	}
	if f.Error != "" {
		a.render(w, r, http.StatusUnprocessableEntity, "profile", f)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	if !f.ParallelEntries {
		var timers int
		if err := tx.QueryRow(`SELECT count(*) FROM time_entry WHERE user_id = ? AND end_time IS NULL`, u.ID).Scan(&timers); err != nil {
			a.serverError(w, r, err)
			return
		}
		if timers > 1 {
			f.ParallelEntries = true
			f.Error = "settings.error.parallel_entries"
			a.render(w, r, http.StatusUnprocessableEntity, "profile", f)
			return
		}
	}
	if _, err := tx.Exec(`UPDATE user SET language = ?, timezone = ?, parallel_entries = ? WHERE id = ?`,
		f.Language, f.Timezone, f.ParallelEntries, u.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := tx.Commit(); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/profile", http.StatusSeeOther)
}

// validTimezone accepts IANA names only; LoadLocation also takes "" and "Local".
func validTimezone(name string) bool {
	if name == "" || name == "Local" {
		return false
	}
	_, err := time.LoadLocation(name)
	return err == nil
}

func (f profileForm) DefaultLanguageName() string { return languageNames[f.Default.Language] }
