package app

import (
	"net/http"
	"slices"
	"strconv"
	"strings"

	"github.com/da-wilky/compact-company/internal/config"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("POST /settings/business", a.authed(a.businessSave))
	})
}

type businessPage struct {
	Business config.Business
	VATRate  string
	Managed  bool
	Missing  []string
	Errors   []string
}

func (p businessPage) TaxModes() []string { return config.TaxModes }

func (p businessPage) AddressText() string { return strings.Join(p.Business.Address, "\n") }

// loadBusiness returns the business data invoices use: config's when it manages it, else the stored row.
func (a *App) loadBusiness() (config.Business, error) {
	if a.cfg.BusinessManaged {
		return a.cfg.Business, nil
	}
	var b config.Business
	var address string
	err := a.db.QueryRow(`SELECT name, company, address, email, web, phone, tax_number,
		bank_name, bank_iban, bank_bic, tax_mode, vat_rate FROM business`).
		Scan(&b.Name, &b.Company, &address, &b.Email, &b.Web, &b.Phone, &b.TaxNumber,
			&b.Bank.Name, &b.Bank.IBAN, &b.Bank.BIC, &b.TaxMode, &b.VATRate)
	b.Address = addressLines(address)
	return b, err
}

// missingBusinessFields returns the catalog keys of the fields an invoice cannot be previewed or issued without.
func missingBusinessFields(b config.Business) []string {
	var missing []string
	if strings.TrimSpace(b.Name) == "" {
		missing = append(missing, "business.name")
	}
	if len(b.Address) == 0 {
		missing = append(missing, "business.address")
	}
	if strings.TrimSpace(b.TaxNumber) == "" {
		missing = append(missing, "business.tax_number")
	}
	return missing
}

func addressLines(s string) []string {
	var lines []string
	for line := range strings.Lines(s) {
		if line = strings.TrimSpace(line); line != "" {
			lines = append(lines, line)
		}
	}
	return lines
}

func (a *App) renderBusiness(w http.ResponseWriter, r *http.Request, status int, b businessPage) {
	p, err := a.loadSettings()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	p.Business = a.businessSection(b)
	a.render(w, r, status, "settings", p)
}

func (a *App) businessSection(p businessPage) businessPage {
	p.Managed = a.cfg.BusinessManaged
	p.Missing = missingBusinessFields(p.Business)
	return p
}

func (a *App) businessSave(w http.ResponseWriter, r *http.Request) {
	if a.cfg.BusinessManaged {
		a.renderBusiness(w, r, http.StatusConflict, businessPage{Business: a.cfg.Business, VATRate: strconv.Itoa(a.cfg.Business.VATRate),
			Errors: []string{"business.error.managed"}})
		return
	}
	p := businessPage{
		Business: config.Business{
			Name:      strings.TrimSpace(r.PostFormValue("name")),
			Company:   strings.TrimSpace(r.PostFormValue("company")),
			Address:   addressLines(r.PostFormValue("address")),
			Email:     strings.TrimSpace(r.PostFormValue("email")),
			Web:       strings.TrimSpace(r.PostFormValue("web")),
			Phone:     strings.TrimSpace(r.PostFormValue("phone")),
			TaxNumber: strings.TrimSpace(r.PostFormValue("tax_number")),
			Bank: config.Bank{
				Name: strings.TrimSpace(r.PostFormValue("bank_name")),
				IBAN: strings.TrimSpace(r.PostFormValue("bank_iban")),
				BIC:  strings.TrimSpace(r.PostFormValue("bank_bic")),
			},
			TaxMode: r.PostFormValue("tax_mode"),
		},
		VATRate: strings.TrimSpace(r.PostFormValue("vat_rate")),
	}
	if !slices.Contains(config.TaxModes, p.Business.TaxMode) {
		p.Errors = append(p.Errors, "business.error.tax_mode")
	}
	rate, err := strconv.Atoi(p.VATRate)
	if err != nil || rate < 0 || rate > 100 {
		p.Errors = append(p.Errors, "business.error.vat_rate")
	}
	if p.Errors != nil {
		a.renderBusiness(w, r, http.StatusUnprocessableEntity, p)
		return
	}
	b := p.Business
	if _, err := a.db.Exec(`UPDATE business SET name = ?, company = ?, address = ?, email = ?, web = ?, phone = ?,
		tax_number = ?, bank_name = ?, bank_iban = ?, bank_bic = ?, tax_mode = ?, vat_rate = ?`,
		b.Name, b.Company, strings.Join(b.Address, "\n"), b.Email, b.Web, b.Phone,
		b.TaxNumber, b.Bank.Name, b.Bank.IBAN, b.Bank.BIC, b.TaxMode, rate); err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, "/settings", http.StatusSeeOther)
}
