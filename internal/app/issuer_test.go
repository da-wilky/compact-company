package app_test

import (
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"testing"
	"time"
)

const (
	testClientID     = "compact-company"
	testClientSecret = "s3cret"
)

// issuer is a stub OIDC provider serving discovery, JWKS and token endpoints.
type issuer struct {
	*httptest.Server
	t     *testing.T
	key   *rsa.PrivateKey
	mu    sync.Mutex
	codes map[string]grant
}

type grant struct {
	claims        map[string]any
	codeChallenge string
	redirectURI   string
}

func newIssuer(t *testing.T) *issuer {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	iss := &issuer{t: t, key: key, codes: map[string]grant{}}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /.well-known/openid-configuration", iss.discovery)
	mux.HandleFunc("GET /jwks", iss.jwks)
	mux.HandleFunc("POST /token", iss.token)
	iss.Server = httptest.NewServer(mux)
	t.Cleanup(iss.Close)
	return iss
}

// config is a valid OIDC-mode config against this issuer; append keys to its [auth] table.
func (iss *issuer) config(t *testing.T, allowedSubjects ...string) string {
	t.Helper()
	secret := filepath.Join(t.TempDir(), "client-secret")
	if err := os.WriteFile(secret, []byte(testClientSecret+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	allowed, _ := json.Marshal(allowedSubjects)
	return `
listen = "0.0.0.0:8080"
public_url = "` + baseURL + `"

[auth]
mode = "oidc"
issuer = ` + strconv.Quote(iss.URL) + `
client_id = "` + testClientID + `"
client_secret_file = ` + strconv.Quote(secret) + `
allowed_subjects = ` + string(allowed) + `
`
}

func (iss *issuer) discovery(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, map[string]any{
		"issuer":                                iss.URL,
		"authorization_endpoint":                iss.URL + "/authorize",
		"token_endpoint":                        iss.URL + "/token",
		"jwks_uri":                              iss.URL + "/jwks",
		"id_token_signing_alg_values_supported": []string{"RS256"},
	})
}

func (iss *issuer) jwks(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, map[string]any{"keys": []map[string]string{{
		"kty": "RSA",
		"kid": "k1",
		"alg": "RS256",
		"use": "sig",
		"n":   b64(iss.key.N.Bytes()),
		"e":   b64(big.NewInt(int64(iss.key.E)).Bytes()),
	}}})
}

// authorize plays the user's visit to the authorization endpoint: it takes the
// app's redirect to the issuer and returns the callback URL the browser is sent back to.
func (iss *issuer) authorize(authURL, subject, name, email string) string {
	iss.t.Helper()
	u, err := url.Parse(authURL)
	if err != nil {
		iss.t.Fatal(err)
	}
	q := u.Query()
	if u.Scheme+"://"+u.Host+u.Path != iss.URL+"/authorize" {
		iss.t.Fatalf("redirected to %s, want the issuer's authorization endpoint", authURL)
	}
	if q.Get("client_id") != testClientID || q.Get("response_type") != "code" || q.Get("code_challenge_method") != "S256" {
		iss.t.Fatalf("unexpected authorization request %s", authURL)
	}
	code := rand.Text()
	iss.mu.Lock()
	iss.codes[code] = grant{
		claims:        map[string]any{"sub": subject, "name": name, "email": email, "nonce": q.Get("nonce")},
		codeChallenge: q.Get("code_challenge"),
		redirectURI:   q.Get("redirect_uri"),
	}
	iss.mu.Unlock()
	cb, err := url.Parse(q.Get("redirect_uri"))
	if err != nil {
		iss.t.Fatal(err)
	}
	cb.RawQuery = url.Values{"code": {code}, "state": {q.Get("state")}}.Encode()
	return cb.String()
}

func (iss *issuer) token(w http.ResponseWriter, r *http.Request) {
	id, secret, ok := r.BasicAuth()
	if !ok || id != testClientID || secret != testClientSecret {
		http.Error(w, `{"error":"invalid_client"}`, http.StatusUnauthorized)
		return
	}
	iss.mu.Lock()
	g, ok := iss.codes[r.PostFormValue("code")]
	delete(iss.codes, r.PostFormValue("code"))
	iss.mu.Unlock()
	verifier := sha256.Sum256([]byte(r.PostFormValue("code_verifier")))
	if !ok || b64(verifier[:]) != g.codeChallenge || r.PostFormValue("redirect_uri") != g.redirectURI {
		http.Error(w, `{"error":"invalid_grant"}`, http.StatusBadRequest)
		return
	}
	claims := map[string]any{
		"iss": iss.URL,
		"aud": testClientID,
		"iat": time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC).Unix(),
		"exp": time.Date(2100, 1, 1, 0, 0, 0, 0, time.UTC).Unix(),
	}
	for k, v := range g.claims {
		claims[k] = v
	}
	writeJSON(w, map[string]any{
		"access_token": rand.Text(),
		"token_type":   "Bearer",
		"expires_in":   300,
		"id_token":     iss.sign(claims),
	})
}

func (iss *issuer) sign(claims map[string]any) string {
	header, _ := json.Marshal(map[string]string{"alg": "RS256", "kid": "k1", "typ": "JWT"})
	payload, _ := json.Marshal(claims)
	signed := b64(header) + "." + b64(payload)
	digest := sha256.Sum256([]byte(signed))
	sig, err := rsa.SignPKCS1v15(nil, iss.key, crypto.SHA256, digest[:])
	if err != nil {
		iss.t.Fatal(err)
	}
	return signed + "." + b64(sig)
}

func b64(b []byte) string { return base64.RawURLEncoding.EncodeToString(b) }

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(v)
}

// oidcLogin runs the authorization code flow through the app and the stub issuer.
func (c *client) oidcLogin(iss *issuer, subject, name, email string) *response {
	c.t.Helper()
	res := c.postForm("/login", nil)
	res.wantStatus(c.t, http.StatusSeeOther)
	callback := iss.authorize(res.Header.Get("Location"), subject, name, email)
	return c.do(httptest.NewRequest(http.MethodGet, callback, nil))
}
