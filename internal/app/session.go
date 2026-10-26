package app

import (
	"context"
	"crypto/rand"
	"database/sql"
	"errors"
	"net/http"
	"unicode"
	"unicode/utf8"
)

const sessionCookie = "session"

type User struct {
	ID              int64
	DisplayName     string
	Email           string
	Language        string
	Timezone        string
	ParallelEntries bool
}

func (u *User) Initial() string {
	r, _ := utf8.DecodeRuneInString(u.DisplayName)
	return string(unicode.ToUpper(r))
}

type identity struct {
	Issuer, Subject, DisplayName, Email string
}

type userKey struct{}

func currentUser(r *http.Request) *User {
	u, _ := r.Context().Value(userKey{}).(*User)
	return u
}

// upsertUser creates the user with configured defaults on first sight and
// otherwise refreshes the identity claims only.
func (a *App) upsertUser(id identity) (int64, error) {
	d := a.cfg.Defaults
	var userID int64
	err := a.db.QueryRow(`
		INSERT INTO user (issuer, subject, display_name, email, language, timezone, parallel_entries)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT (issuer, subject) DO UPDATE SET display_name = excluded.display_name, email = excluded.email
		RETURNING id`,
		id.Issuer, id.Subject, id.DisplayName, id.Email, d.Language, d.Timezone, d.ParallelEntries,
	).Scan(&userID)
	return userID, err
}

func (a *App) login(w http.ResponseWriter, id identity) error {
	userID, err := a.upsertUser(id)
	if err != nil {
		return err
	}
	return a.startSession(w, userID)
}

func (a *App) purgeSessions() error {
	_, err := a.db.Exec(`DELETE FROM session WHERE last_seen < ?`, a.now().Unix()-int64(a.cfg.Auth.SessionIdleTimeout.Seconds()))
	return err
}

func (a *App) startSession(w http.ResponseWriter, userID int64) error {
	if err := a.purgeSessions(); err != nil {
		return err
	}
	sid := rand.Text()
	if _, err := a.db.Exec(`INSERT INTO session (id, user_id, last_seen) VALUES (?, ?, ?)`,
		sid, userID, a.now().Unix()); err != nil {
		return err
	}
	a.setSessionCookie(w, sid)
	return nil
}

func (a *App) setSessionCookie(w http.ResponseWriter, sid string) {
	http.SetCookie(w, &http.Cookie{
		Name:     sessionCookie,
		Value:    sid,
		Path:     "/",
		MaxAge:   int(a.cfg.Auth.SessionIdleTimeout.Seconds()),
		Secure:   true,
		HttpOnly: true,
		SameSite: http.SameSiteLaxMode,
	})
}

const userColumns = `u.id, u.display_name, u.email, u.language, u.timezone, u.parallel_entries`

func scanUser(row *sql.Row, extra ...any) (*User, error) {
	var u User
	dest := append([]any{&u.ID, &u.DisplayName, &u.Email, &u.Language, &u.Timezone, &u.ParallelEntries}, extra...)
	if err := row.Scan(dest...); err != nil {
		return nil, err
	}
	return &u, nil
}

func (a *App) userByID(id int64) (*User, error) {
	return scanUser(a.db.QueryRow(`SELECT `+userColumns+` FROM user u WHERE u.id = ?`, id))
}

// sessionUser returns the user of a live session and touches it; idle sessions are deleted.
func (a *App) sessionUser(w http.ResponseWriter, r *http.Request) (*User, error) {
	ck, err := r.Cookie(sessionCookie)
	if err != nil {
		return nil, nil
	}
	var lastSeen int64
	var subject string
	u, err := scanUser(a.db.QueryRow(`
		SELECT `+userColumns+`, s.last_seen, u.subject
		FROM session s JOIN user u ON u.id = s.user_id
		WHERE s.id = ?`, ck.Value), &lastSeen, &subject)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	now := a.now().Unix()
	// Dev mode skips the admission check so test sessions of arbitrary users keep working.
	revoked := a.oidc != nil && !a.oidc.allowed[subject]
	if revoked || now-lastSeen > int64(a.cfg.Auth.SessionIdleTimeout.Seconds()) {
		_, err := a.db.Exec(`DELETE FROM session WHERE id = ?`, ck.Value)
		return nil, err
	}
	if _, err := a.db.Exec(`UPDATE session SET last_seen = ? WHERE id = ?`, now, ck.Value); err != nil {
		return nil, err
	}
	a.setSessionCookie(w, ck.Value)
	return u, nil
}

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /logout", a.logout)
	})
}

func (a *App) logout(w http.ResponseWriter, r *http.Request) {
	if ck, err := r.Cookie(sessionCookie); err == nil {
		if _, err := a.db.Exec(`DELETE FROM session WHERE id = ?`, ck.Value); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	http.SetCookie(w, &http.Cookie{Name: sessionCookie, Path: "/", MaxAge: -1, Secure: true, HttpOnly: true, SameSite: http.SameSiteLaxMode})
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

// authed puts the session's user into the request context. Without a session,
// auth mode "none" logs in as the dev user; otherwise the request goes to login.
func (a *App) authed(h http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		u, err := a.sessionUser(w, r)
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		if u == nil {
			if a.cfg.Auth.Mode != "none" {
				http.Redirect(w, r, "/login", http.StatusSeeOther)
				return
			}
			if err := a.startSession(w, a.devUserID); err != nil {
				a.serverError(w, r, err)
				return
			}
			if u, err = a.userByID(a.devUserID); err != nil {
				a.serverError(w, r, err)
				return
			}
		}
		h(w, r.WithContext(context.WithValue(r.Context(), userKey{}, u)))
	}
}
