package app

import "net/http"

const TestIssuer = "https://issuer.test"

func (a *App) LoginForTest(w http.ResponseWriter, subject, displayName, email string) error {
	return a.login(w, identity{Issuer: TestIssuer, Subject: subject, DisplayName: displayName, Email: email})
}

// SendQuoteForTest numbers a quote without rendering it, until sending exists.
func (a *App) SendQuoteForTest(id int64, number, sendDate string) error {
	_, err := a.db.Exec(`UPDATE quote SET number = ?, send_date = ? WHERE id = ?`, number, sendDate, id)
	return err
}

func (a *App) SessionCountForTest() (n int, err error) {
	err = a.db.QueryRow(`SELECT count(*) FROM session`).Scan(&n)
	return n, err
}
