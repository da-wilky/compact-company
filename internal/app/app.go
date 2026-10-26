package app

import (
	"context"
	"database/sql"
	"embed"
	"fmt"
	"html/template"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
	_ "time/tzdata"

	"golang.org/x/text/message/catalog"

	"github.com/da-wilky/compact-company/internal/config"
	"github.com/da-wilky/compact-company/internal/db"
)

//go:embed static
var static embed.FS

type App struct {
	cfg         config.Config
	db          *sql.DB
	now         func() time.Time
	handler     http.Handler
	pages       map[string]*template.Template
	catalog     catalog.Catalog
	devUserID   int64
	oidc        *oidcClient
	templateDir string
}

// routes is appended to by each feature file's init, so features register
// their handlers without touching a shared list.
var routes []func(a *App, mux *http.ServeMux)

func New(cfg config.Config, now func() time.Time) (*App, error) {
	if !supportedLanguage(cfg.Defaults.Language) {
		return nil, fmt.Errorf("defaults.language: unsupported language %q (want one of %v)", cfg.Defaults.Language, languages)
	}
	cat, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	pages, err := parsePages()
	if err != nil {
		return nil, err
	}
	var oidcc *oidcClient
	if cfg.Auth.Mode == "oidc" {
		if oidcc, err = newOIDCClient(cfg, now); err != nil {
			return nil, err
		}
	}
	if err := os.MkdirAll(cfg.DataDir, 0o750); err != nil {
		return nil, fmt.Errorf("data dir: %w", err)
	}
	templateDir := cfg.Invoice.TemplateDir
	if templateDir == "" {
		if templateDir, err = extractDefaultTemplate(cfg.DataDir); err != nil {
			return nil, fmt.Errorf("default template: %w", err)
		}
	}
	database, err := db.Open(filepath.Join(cfg.DataDir, "app.db"))
	if err != nil {
		return nil, fmt.Errorf("database: %w", err)
	}
	a := &App{cfg: cfg, db: database, now: now, pages: pages, catalog: cat, oidc: oidcc, templateDir: templateDir}
	if err := a.purgeSessions(); err != nil {
		_ = database.Close()
		return nil, fmt.Errorf("purge sessions: %w", err)
	}
	if cfg.Auth.Mode == "none" {
		a.devUserID, err = a.upsertUser(identity{Issuer: "none", Subject: "dev", DisplayName: "dev"})
		if err != nil {
			_ = database.Close()
			return nil, fmt.Errorf("dev user: %w", err)
		}
	}

	mux := http.NewServeMux()
	mux.Handle("GET /static/", http.FileServerFS(static))
	for _, register := range routes {
		register(a, mux)
	}
	cop := http.NewCrossOriginProtection()
	cop.SetDenyHandler(http.HandlerFunc(a.forbidden))
	a.handler = cop.Handler(a.unmatched(mux))
	return a, nil
}

// unmatched replaces the mux's plain-text 404 and 405 responses with the error page.
func (a *App) unmatched(mux *http.ServeMux) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if _, pattern := mux.Handler(r); pattern != "" {
			mux.ServeHTTP(w, r)
			return
		}
		var allow []string
		for _, m := range []string{http.MethodGet, http.MethodPost} {
			probe := r.Clone(r.Context())
			probe.Method = m
			if _, pattern := mux.Handler(probe); pattern != "" {
				allow = append(allow, m)
			}
		}
		u, err := a.sessionUser(w, r)
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		if u != nil {
			r = r.WithContext(context.WithValue(r.Context(), userKey{}, u))
		}
		if allow == nil {
			a.notFound(w, r)
			return
		}
		w.Header().Set("Allow", strings.Join(allow, ", "))
		a.errorPage(w, r, http.StatusMethodNotAllowed, "error.method_not_allowed")
	})
}

func (a *App) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	a.handler.ServeHTTP(w, r)
}

func (a *App) Close() error {
	return a.db.Close()
}

func (a *App) serverError(w http.ResponseWriter, r *http.Request, err error) {
	slog.Error("request failed", "method", r.Method, "path", r.URL.Path, "err", err)
	a.errorPage(w, r, http.StatusInternalServerError, "error.server")
}

func (a *App) notFound(w http.ResponseWriter, r *http.Request) {
	a.errorPage(w, r, http.StatusNotFound, "error.not_found")
}

func (a *App) forbidden(w http.ResponseWriter, r *http.Request) {
	a.errorPage(w, r, http.StatusForbidden, "error.forbidden")
}

// errorPage falls back to plain text so a failing render can't recurse into serverError.
func (a *App) errorPage(w http.ResponseWriter, r *http.Request, status int, key string) {
	if err := a.write(w, r, status, "error", "layout", view{Data: key}); err != nil {
		slog.Error("error page failed", "path", r.URL.Path, "err", err)
		http.Error(w, http.StatusText(status), status)
	}
}
