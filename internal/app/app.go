package app

import (
	"database/sql"
	"embed"
	"fmt"
	"html/template"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"time"
	_ "time/tzdata"

	"golang.org/x/text/message/catalog"

	"github.com/da-wilky/compact-company/internal/config"
	"github.com/da-wilky/compact-company/internal/db"
)

//go:embed static
var static embed.FS

type App struct {
	cfg       config.Config
	db        *sql.DB
	now       func() time.Time
	handler   http.Handler
	pages     map[string]*template.Template
	catalog   catalog.Catalog
	devUserID int64
	oidc      *oidcClient
}

// routes is appended to by each feature file's init, so features register
// their handlers without touching a shared list.
var routes []func(a *App, mux *http.ServeMux)

func New(cfg config.Config, now func() time.Time) (*App, error) {
	if !supportedLanguage(cfg.Defaults.Language) {
		return nil, fmt.Errorf("defaults.language: unsupported language %q (want one of %v)", cfg.Defaults.Language, languages)
	}
	if _, err := formatCustomerNumber(cfg.CustomerNumberFormat, 1); err != nil {
		return nil, err
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
	database, err := db.Open(filepath.Join(cfg.DataDir, "app.db"))
	if err != nil {
		return nil, fmt.Errorf("database: %w", err)
	}
	a := &App{cfg: cfg, db: database, now: now, pages: pages, catalog: cat, oidc: oidcc}
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
	a.handler = http.NewCrossOriginProtection().Handler(mux)
	return a, nil
}

func (a *App) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	a.handler.ServeHTTP(w, r)
}

func (a *App) Close() error {
	return a.db.Close()
}

func (a *App) serverError(w http.ResponseWriter, r *http.Request, err error) {
	slog.Error("request failed", "method", r.Method, "path", r.URL.Path, "err", err)
	http.Error(w, http.StatusText(http.StatusInternalServerError), http.StatusInternalServerError)
}
