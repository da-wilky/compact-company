package app_test

import (
	"html"
	"io"
	"net/http"
	"net/http/cookiejar"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"slices"
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

// oidcConfig is a valid OIDC-mode config against a fresh stub issuer; append keys to its [auth] table.
func oidcConfig(t *testing.T) string {
	t.Helper()
	return newIssuer(t).config(t, "alice")
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
	cfg   config.Config
	app   *app.App
	clock *clock
}

// startApp boots the app the way serve does: config file on disk, then app.New.
// data_dir is prepended so configTOML must not set it.
func startApp(t *testing.T, configTOML string, c *clock) (*app.App, error) {
	t.Helper()
	cfg, err := loadConfig(t, configTOML)
	if err != nil {
		return nil, err
	}
	return app.New(cfg, c.Now)
}

func loadConfig(t *testing.T, configTOML string) (config.Config, error) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	content := "data_dir = " + strconv.Quote(dir) + "\n" + configTOML
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return config.Load(path)
}

func newTestApp(t *testing.T, configTOML string) *testApp {
	t.Helper()
	cfg, err := loadConfig(t, configTOML)
	if err != nil {
		t.Fatalf("load config: %v", err)
	}
	ta := &testApp{t: t, cfg: cfg, clock: &clock{now: time.Date(2026, 10, 26, 9, 0, 0, 0, time.UTC)}}
	ta.start()
	t.Cleanup(func() { _ = ta.app.Close() })
	return ta
}

func (ta *testApp) start() {
	ta.t.Helper()
	a, err := app.New(ta.cfg, ta.clock.Now)
	if err != nil {
		ta.t.Fatalf("start app: %v", err)
	}
	ta.app = a
}

// restart closes the app and starts a new one on the same config and data dir.
func (ta *testApp) restart() {
	ta.t.Helper()
	if err := ta.app.Close(); err != nil {
		ta.t.Fatal(err)
	}
	ta.start()
}

type client struct {
	t   *testing.T
	ta  *testApp
	jar http.CookieJar
}

func (ta *testApp) client() *client {
	jar, err := cookiejar.New(nil)
	if err != nil {
		ta.t.Fatal(err)
	}
	return &client{t: ta.t, ta: ta, jar: jar}
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
	c.ta.app.ServeHTTP(rec, req)
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

var (
	tableRow  = regexp.MustCompile(`(?s)<tr[ >].*?</tr>`)
	tableCell = regexp.MustCompile(`(?s)<t[dh][ >].*?</t[dh]>`)
	markupTag = regexp.MustCompile(`<[^>]*>`)
)

// textOf reads markup as a user does: tags gone, whitespace collapsed.
func textOf(markup string) string {
	return strings.Join(strings.Fields(html.UnescapeString(markupTag.ReplaceAllString(markup, " "))), " ")
}

func (r *response) text() string {
	return textOf(r.body)
}

// in narrows r to the page section under the h2 that reads heading, up to the next h2.
func (r *response) in(t *testing.T, heading string) *response {
	t.Helper()
	for _, part := range strings.Split(r.body, "<h2")[1:] {
		head, rest, ok := strings.Cut(part, "</h2>")
		if ok && textOf("<h2"+head) == heading {
			return &response{Response: r.Response, body: rest}
		}
	}
	t.Fatalf("no section headed %q:\n%s", heading, r.body)
	return nil
}

func (r *response) rows() [][]string {
	var rows [][]string
	for _, row := range tableRow.FindAllString(r.body, -1) {
		var cells []string
		for _, cell := range tableCell.FindAllString(row, -1) {
			cells = append(cells, textOf(cell))
		}
		rows = append(rows, cells)
	}
	return rows
}

func (r *response) wantRow(t *testing.T, cells ...string) {
	t.Helper()
	rows := r.rows()
	if !slices.ContainsFunc(rows, func(row []string) bool { return slices.Equal(row, cells) }) {
		t.Fatalf("no table row reads %q; rows:\n%q", cells, rows)
	}
}
