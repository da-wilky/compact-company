package app_test

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
)

func TestCrossOriginFormSubmissionIsRejected(t *testing.T) {
	ta := newTestApp(t, devConfig)
	req := httptest.NewRequest(http.MethodPost, baseURL+"/", nil)
	req.Header.Set("Sec-Fetch-Site", "cross-site")

	res := ta.client().do(req)

	res.wantStatus(t, http.StatusForbidden)
}

func TestSameOriginFormSubmissionPassesCrossOriginProtection(t *testing.T) {
	ta := newTestApp(t, devConfig)

	res := ta.client().postForm("/", url.Values{})

	if res.StatusCode == http.StatusForbidden {
		t.Fatalf("same-origin POST was rejected: %s", res.body)
	}
}

func TestAssetsAreEmbeddedAndServedLocally(t *testing.T) {
	ta := newTestApp(t, devConfig)
	c := ta.client()

	home := c.get("/")
	home.wantBody(t, `src="/static/htmx.min.js"`)
	home.wantBody(t, `href="/static/app.css"`)
	home.wantNoBody(t, "https://")

	htmx := c.get("/static/htmx.min.js")
	htmx.wantStatus(t, http.StatusOK)
	htmx.wantBody(t, "htmx")
	c.get("/static/app.css").wantStatus(t, http.StatusOK)
}

func TestSessionsAndUsersSurviveRestart(t *testing.T) {
	ta := newTestApp(t, devConfig)
	alice := ta.loginAs("alice")

	ta.restart()

	alice.get("/").wantBody(t, "Signed in as alice")
}
