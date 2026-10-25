package app_test

import (
	"io"
	"net/http"
	"net/http/cookiejar"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/da-wilky/compact-company/internal/app"
	"github.com/da-wilky/compact-company/internal/config"
)

const baseURL = "https://compact.test"

const devConfig = `
listen = "127.0.0.1:8080"

[auth]
mode = "none"
`

// oidcConfig is a valid OIDC-mode config; append keys to its [auth] table.
func oidcConfig(t *testing.T) string {
	t.Helper()
	secret := filepath.Join(t.TempDir(), "client-secret")
	if err := os.WriteFile(secret, []byte("s3cret\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	return `
listen = "0.0.0.0:8080"

[auth]
mode = "oidc"
issuer = "https://issuer.test"
client_id = "compact-company"
client_secret_file = ` + strconv.Quote(secret) + `
allowed_subjects = ["alice"]
`
}

type clock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *clock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *clock) Advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = c.now.Add(d)
}

type testApp struct {
	t     *testing.T
	app   *app.App
	clock *clock
}

// startApp boots the app the way serve does: config file on disk, then app.New.
// data_dir is prepended so configTOML must not set it.
func startApp(t *testing.T, configTOML string, c *clock) (*app.App, error) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	content := "data_dir = " + strconv.Quote(dir) + "\n" + configTOML
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	cfg, err := config.Load(path)
	if err != nil {
		return nil, err
	}
	return app.New(cfg, c.Now)
}

func newTestApp(t *testing.T, configTOML string) *testApp {
	t.Helper()
	c := &clock{now: time.Date(2026, 10, 26, 9, 0, 0, 0, time.UTC)}
	a, err := startApp(t, configTOML, c)
	if err != nil {
		t.Fatalf("start app: %v", err)
	}
	t.Cleanup(func() { _ = a.Close() })
	return &testApp{t: t, app: a, clock: c}
}

type client struct {
	t   *testing.T
	h   http.Handler
	jar http.CookieJar
}

func (ta *testApp) client() *client {
	jar, err := cookiejar.New(nil)
	if err != nil {
		ta.t.Fatal(err)
	}
	return &client{t: ta.t, h: ta.app, jar: jar}
}

// loginAs logs in through the real session machinery, bypassing only OIDC.
func (ta *testApp) loginAs(subject string) *client {
	ta.t.Helper()
	c := ta.client()
	rec := httptest.NewRecorder()
	if err := ta.app.LoginForTest(rec, subject, subject, subject+"@example.com"); err != nil {
		ta.t.Fatalf("login as %s: %v", subject, err)
	}
	u, _ := url.Parse(baseURL)
	c.jar.SetCookies(u, rec.Result().Cookies())
	return c
}

type response struct {
	*http.Response
	body string
}

func (c *client) do(req *http.Request) *response {
	c.t.Helper()
	for _, ck := range c.jar.Cookies(req.URL) {
		req.AddCookie(ck)
	}
	rec := httptest.NewRecorder()
	c.h.ServeHTTP(rec, req)
	res := rec.Result()
	c.jar.SetCookies(req.URL, res.Cookies())
	body, err := io.ReadAll(res.Body)
	if err != nil {
		c.t.Fatal(err)
	}
	return &response{Response: res, body: string(body)}
}

func (c *client) get(path string) *response {
	c.t.Helper()
	return c.do(httptest.NewRequest(http.MethodGet, baseURL+path, nil))
}

// postForm sends a same-origin form submission, as a browser would.
func (c *client) postForm(path string, form url.Values) *response {
	c.t.Helper()
	req := httptest.NewRequest(http.MethodPost, baseURL+path, strings.NewReader(form.Encode()))
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Sec-Fetch-Site", "same-origin")
	return c.do(req)
}

func (r *response) wantStatus(t *testing.T, code int) {
	t.Helper()
	if r.StatusCode != code {
		t.Fatalf("status = %d, want %d; body:\n%s", r.StatusCode, code, r.body)
	}
}

func (r *response) wantBody(t *testing.T, substr string) {
	t.Helper()
	if !strings.Contains(r.body, substr) {
		t.Fatalf("body does not contain %q:\n%s", substr, r.body)
	}
}

func (r *response) wantNoBody(t *testing.T, substr string) {
	t.Helper()
	if strings.Contains(r.body, substr) {
		t.Fatalf("body unexpectedly contains %q:\n%s", substr, r.body)
	}
}
