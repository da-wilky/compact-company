package app

import (
	"net/http"
	"time"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /settings", a.authed(a.settingsPage))
		mux.HandleFunc("POST /settings", a.authed(a.settingsSave))
	})
}

var languageNames = map[string]string{"en": "English", "de": "Deutsch"}

type settingsForm struct {
	Language string
	Timezone string
	Error    string
	Now      time.Time
}

type settingsOption struct {
	Value, Name string
}

func (f settingsForm) Languages() []settingsOption {
	opts := make([]settingsOption, len(languages))
	for i, tag := range languages {
		opts[i] = settingsOption{tag.String(), languageNames[tag.String()]}
	}
	return opts
}

func (a *App) settingsPage(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	a.render(w, r, http.StatusOK, "settings", settingsForm{Language: u.Language, Timezone: u.Timezone, Now: a.now()})
}

func (a *App) settingsSave(w http.ResponseWriter, r *http.Request) {
	u := currentUser(r)
	f := settingsForm{Language: r.PostFormValue("language"), Timezone: r.PostFormValue("timezone"), Now: a.now()}
	switch {
	case !supportedLanguage(f.Language):
		f.Error = "settings.error.language"
	case !validTimezone(f.Timezone):
		f.Error = "settings.error.timezone"
	}
	if f.Error != "" {
		a.render(w, r, http.StatusUnprocessableEntity, "settings", f)
		return
	}
	if _, err := a.db.Exec(`UPDATE user SET language = ?, timezone = ? WHERE id = ?`, f.Language, f.Timezone, u.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/settings", http.StatusSeeOther)
}

// validTimezone accepts IANA names only; LoadLocation also takes "" and "Local".
func validTimezone(name string) bool {
	if name == "" || name == "Local" {
		return false
	}
	_, err := time.LoadLocation(name)
	return err == nil
}
