package app

import "net/http"

const TestIssuer = "https://issuer.test"

func (a *App) LoginForTest(w http.ResponseWriter, subject, displayName, email string) error {
	return a.login(w, identity{Issuer: TestIssuer, Subject: subject, DisplayName: displayName, Email: email})
}
