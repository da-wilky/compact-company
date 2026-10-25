package app

import (
	"cmp"
	"context"
	"crypto/rand"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/coreos/go-oidc/v3/oidc"
	"golang.org/x/oauth2"

	"github.com/da-wilky/compact-company/internal/config"
)

const loginCookie = "login"

type oidcClient struct {
	issuer   string
	allowed  map[string]bool
	oauth    oauth2.Config
	verifier *oidc.IDTokenVerifier
}

func newOIDCClient(cfg config.Config, now func() time.Time) (*oidcClient, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	provider, err := oidc.NewProvider(ctx, cfg.Auth.Issuer)
	if err != nil {
		return nil, fmt.Errorf("auth.issuer: discovery: %w", err)
	}
	allowed := map[string]bool{}
	for _, s := range cfg.Auth.AllowedSubjects {
		allowed[s] = true
	}
	return &oidcClient{
		issuer:  cfg.Auth.Issuer,
		allowed: allowed,
		oauth: oauth2.Config{
			ClientID:     cfg.Auth.ClientID,
			ClientSecret: cfg.Auth.ClientSecret,
			Endpoint:     provider.Endpoint(),
			RedirectURL:  cfg.PublicURL + "/auth/callback",
			Scopes:       []string{oidc.ScopeOpenID, "profile", "email"},
		},
		verifier: provider.Verifier(&oidc.Config{ClientID: cfg.Auth.ClientID, Now: now}),
	}, nil
}

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		if a.oidc == nil {
			return
		}
		mux.HandleFunc("GET /login", func(w http.ResponseWriter, r *http.Request) {
			a.render(w, r, http.StatusOK, "login", nil)
		})
		mux.HandleFunc("POST /login", a.loginStart)
		mux.HandleFunc("GET /auth/callback", a.loginCallback)
	})
}

func (a *App) loginStart(w http.ResponseWriter, r *http.Request) {
	state, nonce, verifier := rand.Text(), rand.Text(), oauth2.GenerateVerifier()
	http.SetCookie(w, &http.Cookie{
		Name:     loginCookie,
		Value:    state + "." + nonce + "." + verifier,
		Path:     "/auth/callback",
		MaxAge:   600,
		Secure:   true,
		HttpOnly: true,
		SameSite: http.SameSiteLaxMode,
	})
	http.Redirect(w, r, a.oidc.oauth.AuthCodeURL(state, oidc.Nonce(nonce), oauth2.S256ChallengeOption(verifier)), http.StatusSeeOther)
}

func (a *App) loginCallback(w http.ResponseWriter, r *http.Request) {
	ck, err := r.Cookie(loginCookie)
	if err != nil {
		loginFailed(w, err)
		return
	}
	http.SetCookie(w, &http.Cookie{Name: loginCookie, Path: "/auth/callback", MaxAge: -1, Secure: true, HttpOnly: true})
	parts := strings.Split(ck.Value, ".")
	if len(parts) != 3 || r.URL.Query().Get("state") != parts[0] {
		loginFailed(w, fmt.Errorf("state mismatch"))
		return
	}
	nonce, verifier := parts[1], parts[2]
	token, err := a.oidc.oauth.Exchange(r.Context(), r.URL.Query().Get("code"), oauth2.VerifierOption(verifier))
	if err != nil {
		loginFailed(w, err)
		return
	}
	raw, _ := token.Extra("id_token").(string)
	idToken, err := a.oidc.verifier.Verify(r.Context(), raw)
	if err != nil {
		loginFailed(w, err)
		return
	}
	if idToken.Nonce != nonce {
		loginFailed(w, fmt.Errorf("nonce mismatch"))
		return
	}
	var claims struct {
		Name              string `json:"name"`
		PreferredUsername string `json:"preferred_username"`
		Email             string `json:"email"`
	}
	if err := idToken.Claims(&claims); err != nil {
		loginFailed(w, err)
		return
	}
	if !a.oidc.allowed[idToken.Subject] {
		a.render(w, r, http.StatusForbidden, "not_allowed", idToken.Subject)
		return
	}
	name := cmp.Or(claims.Name, claims.PreferredUsername, idToken.Subject)
	if err := a.login(w, identity{Issuer: a.oidc.issuer, Subject: idToken.Subject, DisplayName: name, Email: claims.Email}); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func loginFailed(w http.ResponseWriter, err error) {
	slog.Warn("login failed", "err", err)
	http.Error(w, "login failed", http.StatusBadRequest)
}
