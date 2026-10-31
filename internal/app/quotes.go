package app

import (
	"database/sql"
	"errors"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /quotes", a.authed(a.quoteList))
		mux.HandleFunc("GET /quotes/new", a.authed(a.quoteNew))
		mux.HandleFunc("POST /quotes", a.authed(a.quoteCreate))
		mux.HandleFunc("GET /quotes/{id}", a.authed(a.quoteShow))
		mux.HandleFunc("POST /quotes/{id}", a.authed(a.quoteUpdate))
		mux.HandleFunc("POST /quotes/{id}/delete", a.authed(a.quoteDelete))
	})
}

var pricings = []string{"fixed", "effort"}

// quoteHeader is a quote's editable fields as submitted; Customer is fixed once the draft exists.
type quoteHeader struct {
	Customer   string
	Project    string
	Subject    string
	Language   string
	Pricing    string
	ValidUntil string
}

type quoteFormOptions struct {
	Projects  []projectOption
	Languages []languageOption
	Pricings  []string
}

func (a *App) quoteFormOptions(customerID string) (quoteFormOptions, error) {
	projects, err := a.projectOptions("p.customer_id = ?", customerID)
	return quoteFormOptions{projects, languageOptions(), pricings}, err
}

type quoteNewPage struct {
	Form      quoteHeader
	Customers []customerOption
	quoteFormOptions
	Errors []string
}

func (a *App) quoteNew(w http.ResponseWriter, r *http.Request) {
	a.renderQuoteNew(w, r, http.StatusOK, quoteHeader{
		Customer: r.FormValue("customer"),
		Project:  r.FormValue("project"),
		Pricing:  "fixed",
	}, nil)
}

// newQuoteValidUntil is the configured validity counted from today in the timezone of customer id.
func (a *App) newQuoteValidUntil(customerID int64) (string, error) {
	loc, err := customerLocation(a.db, "?", customerID)
	if err != nil {
		return "", err
	}
	return a.now().In(loc).AddDate(0, 0, a.cfg.Quote.ValidityDays).Format(time.DateOnly), nil
}

// renderQuoteNew starts the language at the customer's invoice language unless f has one,
// and a fresh form's validity at newQuoteValidUntil.
func (a *App) renderQuoteNew(w http.ResponseWriter, r *http.Request, status int, f quoteHeader, errs []string) {
	customers, err := a.customerOptions()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	// The browser selects the first customer when none of them matches, so its projects are the ones to offer.
	if len(customers) > 0 && !slices.ContainsFunc(customers, func(c customerOption) bool { return strconv.FormatInt(c.ID, 10) == f.Customer }) {
		f.Customer = strconv.FormatInt(customers[0].ID, 10)
	}
	if f.Language == "" && f.Customer != "" {
		if err := a.db.QueryRow(`SELECT invoice_language FROM customer WHERE id = ?`, f.Customer).Scan(&f.Language); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	if errs == nil && f.ValidUntil == "" && f.Customer != "" {
		id, _ := strconv.ParseInt(f.Customer, 10, 64)
		if f.ValidUntil, err = a.newQuoteValidUntil(id); err != nil {
			a.serverError(w, r, err)
			return
		}
	}
	opts, err := a.quoteFormOptions(f.Customer)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "quote_new", quoteNewPage{Form: f, Customers: customers, quoteFormOptions: opts, Errors: errs})
}

func quoteHeaderFromForm(r *http.Request) quoteHeader {
	return quoteHeader{
		Customer:   r.PostFormValue("customer"),
		Project:    r.PostFormValue("project"),
		Subject:    strings.TrimSpace(r.PostFormValue("subject")),
		Language:   r.PostFormValue("language"),
		Pricing:    r.PostFormValue("pricing"),
		ValidUntil: strings.TrimSpace(r.PostFormValue("valid_until")),
	}
}

// validate returns f's project (NULL for none) and its errors as catalog keys; the project must be one of projects.
func (f quoteHeader) validate(projects []projectOption) (sql.NullInt64, []string) {
	var errs []string
	if f.Subject == "" {
		errs = append(errs, "quotes.error.subject")
	}
	var project sql.NullInt64
	if f.Project != "" {
		id, err := strconv.ParseInt(f.Project, 10, 64)
		if err != nil || !slices.ContainsFunc(projects, func(p projectOption) bool { return p.ID == id }) {
			errs = append(errs, "quotes.error.project")
		}
		project = sql.NullInt64{Int64: id, Valid: true}
	}
	if !supportedLanguage(f.Language) {
		errs = append(errs, "quotes.error.language")
	}
	if !slices.Contains(pricings, f.Pricing) {
		errs = append(errs, "quotes.error.pricing")
	}
	if _, err := time.Parse(time.DateOnly, f.ValidUntil); err != nil {
		errs = append(errs, "quotes.error.valid_until")
	}
	return project, errs
}

func (a *App) quoteCreate(w http.ResponseWriter, r *http.Request) {
	f := quoteHeaderFromForm(r)
	customerID, err := strconv.ParseInt(f.Customer, 10, 64)
	var exists bool
	if err == nil {
		err = a.db.QueryRow(`SELECT EXISTS (SELECT 1 FROM customer WHERE id = ?)`, customerID).Scan(&exists)
	}
	if err != nil || !exists {
		a.renderQuoteNew(w, r, http.StatusUnprocessableEntity, f, []string{"invoices.error.customer"})
		return
	}
	projects, err := a.projectOptions("p.customer_id = ?", customerID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	project, errs := f.validate(projects)
	if errs != nil {
		a.renderQuoteNew(w, r, http.StatusUnprocessableEntity, f, errs)
		return
	}
	var id int64
	err = a.db.QueryRow(`INSERT INTO quote (customer_id, project_id, subject, language, currency, pricing, valid_until)
		SELECT id, ?, ?, ?, currency, ?, ? FROM customer WHERE id = ? RETURNING id`,
		project, f.Subject, f.Language, f.Pricing, f.ValidUntil, customerID).Scan(&id)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, quotePath(id), http.StatusSeeOther)
}

func quotePath(id int64) string {
	return "/quotes/" + strconv.FormatInt(id, 10)
}

type quoteDraft struct {
	ID             int64
	CustomerID     int64
	CustomerNumber string
	CustomerName   string
	Currency       string
	Header         quoteHeader
	Lines          []quoteLine
	Totals         []intervalTotals
	VATPercent     int64
	Sections       []quoteSection
	unit           currency.Unit
}

type quotePage struct {
	Quote quoteDraft
	quoteFormOptions
	Snippets []snippet
	Errors   []string
	renderFailure
}

// Form is what the shared quote field partials show.
func (p quotePage) Form() quoteHeader { return p.Quote.Header }

// loadQuoteDraft returns sql.ErrNoRows unless id is a draft.
func (a *App) loadQuoteDraft(id int64) (quoteDraft, error) {
	q := quoteDraft{ID: id}
	var project sql.NullInt64
	err := a.db.QueryRow(`SELECT q.customer_id, c.number, c.name, q.currency, q.project_id, q.subject, q.language, q.pricing, q.valid_until
		FROM quote q JOIN customer c ON c.id = q.customer_id WHERE q.id = ? AND q.number IS NULL`, id).
		Scan(&q.CustomerID, &q.CustomerNumber, &q.CustomerName, &q.Currency, &project,
			&q.Header.Subject, &q.Header.Language, &q.Header.Pricing, &q.Header.ValidUntil)
	if err != nil {
		return q, err
	}
	q.Header.Customer = strconv.FormatInt(q.CustomerID, 10)
	if project.Valid {
		q.Header.Project = strconv.FormatInt(project.Int64, 10)
	}
	q.unit = currency.MustParseISO(q.Currency)
	if q.VATPercent, err = a.draftVATPercent(); err != nil {
		return q, err
	}
	var totals map[string][]int64
	if q.Lines, totals, err = queryQuoteLines(a.db, id, q.unit); err != nil {
		return q, err
	}
	q.Totals = intervalTotalsOf(totals, q.VATPercent, q.unit)
	q.Sections, err = quoteSections(a.db, id)
	return q, err
}

// quoteFromPath writes 404 or 500 itself when ok is false.
func (a *App) quoteFromPath(w http.ResponseWriter, r *http.Request) (quoteDraft, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return quoteDraft{}, false
	}
	q, err := a.loadQuoteDraft(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return q, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return q, false
	}
	return q, true
}

func (a *App) renderQuote(w http.ResponseWriter, r *http.Request, status int, q quoteDraft, errs []string) {
	a.renderQuotePage(w, r, status, quotePage{Quote: q, Errors: errs})
}

func (a *App) renderQuotePage(w http.ResponseWriter, r *http.Request, status int, p quotePage) {
	var err error
	if p.quoteFormOptions, err = a.quoteFormOptions(p.Quote.Header.Customer); err != nil {
		a.serverError(w, r, err)
		return
	}
	if p.Snippets, err = a.snippets(`SELECT s.id, s.name, s.language, s.heading FROM snippet s JOIN quote q ON q.language = s.language
		WHERE q.id = ? ORDER BY s.name, s.id`, p.Quote.ID); err != nil {
		a.serverError(w, r, err)
		return
	}
	if err := checkSections(r.Context(), p.Quote.Sections); err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "quote", p)
}

// renderQuoteFailure shows on the quote's page why q couldn't be rendered.
func (a *App) renderQuoteFailure(w http.ResponseWriter, r *http.Request, q quoteDraft, err error) {
	p := quotePage{Quote: q}
	status, ok := p.explain(err)
	if !ok {
		a.serverError(w, r, err)
		return
	}
	a.renderQuotePage(w, r, status, p)
}

func (a *App) quoteShow(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	q, err := a.loadQuoteDraft(id)
	if err == nil {
		a.renderQuote(w, r, http.StatusOK, q, nil)
		return
	}
	if !errors.Is(err, sql.ErrNoRows) {
		a.serverError(w, r, err)
		return
	}
	sent, err := a.loadSentQuote(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, http.StatusOK, "quote_sent", sent)
}

func (a *App) quoteUpdate(w http.ResponseWriter, r *http.Request) {
	q, ok := a.quoteFromPath(w, r)
	if !ok {
		return
	}
	f := quoteHeaderFromForm(r)
	f.Customer = q.Header.Customer
	projects, err := a.projectOptions("p.customer_id = ?", q.CustomerID)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	project, errs := f.validate(projects)
	q.Header = f
	if errs != nil {
		a.renderQuote(w, r, http.StatusUnprocessableEntity, q, errs)
		return
	}
	res, err := a.db.Exec(`UPDATE quote SET project_id = ?, subject = ?, language = ?, pricing = ?, valid_until = ?
		WHERE id = ? AND number IS NULL`, project, f.Subject, f.Language, f.Pricing, f.ValidUntil, q.ID)
	a.afterInvoiceWrite(w, r, res, err, quotePath(q.ID))
}

// Deleting a draft cascades to its lines and sections.
func (a *App) quoteDelete(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := a.db.Exec(`DELETE FROM quote WHERE id = ? AND number IS NULL`, id)
	a.afterInvoiceWrite(w, r, res, err, "/quotes")
}

// quoteStatus derives a quote's status (quote aliased q) short of expired, which needs today in the customer's timezone: see expire.
const quoteStatus = `CASE WHEN q.number IS NULL THEN 'draft' WHEN q.accepted_date IS NOT NULL THEN 'accepted'
	WHEN q.declined_date IS NOT NULL THEN 'declined' ELSE 'sent' END`

var quoteStatuses = []string{"draft", "sent", "accepted", "declined", "expired"}

// expire makes a sent quote's status expired once today is past validUntil.
func expire(status, validUntil, today string) string {
	if status == "sent" && validUntil < today {
		return "expired"
	}
	return status
}

type quoteRow struct {
	ID       int64
	Number   string
	Customer string
	Subject  string
	SendDate string
	OneOff   string
	Currency string
	Status   string
}

// quoteList is a table of quotes; ShowCustomer adds the customer column where the page isn't one customer's.
type quoteList struct {
	Rows         []quoteRow
	ShowCustomer bool
}

// loadQuoteRows lists the quotes meeting cond (quote aliased q, ? placeholders), newest first: drafts, then by send date.
// A sent quote's one-off total is its snapshot's, so a later VAT change doesn't alter it.
func (a *App) loadQuoteRows(cond string, args ...any) ([]quoteRow, error) {
	rows, err := a.db.Query(`SELECT * FROM (SELECT q.id, ifnull(q.number, ''), c.name, q.subject, ifnull(q.send_date, '') send_date, q.currency,
		`+quoteStatus+` status, q.valid_until, c.timezone, q.number IS NOT NULL sent, ifnull(q.snapshot ->> '$.totals.once.gross', '')
		FROM quote q JOIN customer c ON c.id = q.customer_id WHERE `+cond+`)
		ORDER BY sent, send_date DESC, id DESC`, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var found []quoteRow
	locs := locationCache{}
	for rows.Next() {
		var row quoteRow
		var validUntil, tz string
		var sent bool
		if err := rows.Scan(&row.ID, &row.Number, &row.Customer, &row.Subject, &row.SendDate, &row.Currency, &row.Status, &validUntil, &tz, &sent, &row.OneOff); err != nil {
			return nil, err
		}
		loc, err := locs.load(tz)
		if err != nil {
			return nil, err
		}
		row.Status = expire(row.Status, validUntil, a.today(loc))
		found = append(found, row)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	vat, err := a.draftVATPercent()
	if err != nil {
		return nil, err
	}
	for i := range found {
		if found[i].OneOff != "" {
			continue
		}
		unit := currency.MustParseISO(found[i].Currency)
		_, totals, err := queryQuoteLines(a.db, found[i].ID, unit)
		if err != nil {
			return nil, err
		}
		found[i].OneOff = formatAmount(totalsOf(totals["once"], vat).Gross, unit)
	}
	return found, nil
}

type quotesPage struct {
	List      quoteList
	Customer  int64
	Status    string
	Statuses  filterSelect
	Customers filterSelect
}

// quotesQuery is the list's query for customer and status; 0 and "" mean all.
func quotesQuery(customer int64, status string) url.Values {
	q := url.Values{}
	if customer != 0 {
		q.Set("customer", strconv.FormatInt(customer, 10))
	}
	if status != "" {
		q.Set("status", status)
	}
	return q
}

func (a *App) quoteList(w http.ResponseWriter, r *http.Request) {
	page := quotesPage{List: quoteList{ShowCustomer: true}}
	page.Customer, _ = strconv.ParseInt(r.FormValue("customer"), 10, 64)
	if s := r.FormValue("status"); slices.Contains(quoteStatuses, s) {
		page.Status = s
	}
	var statuses []filterSelectOption
	for _, s := range quoteStatuses {
		statuses = append(statuses, filterSelectOption{Value: s, Key: "quotes.status." + s, Selected: s == page.Status})
	}
	page.Statuses = filterSelect{Action: "/quotes", Name: "status", Label: "quotes.status", All: "quotes.all_statuses",
		Hidden: hiddenFields(quotesQuery(page.Customer, page.Status), "status"), Groups: []filterGroup{{Options: statuses}}}
	customers, err := a.customerOptions()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	var options []filterSelectOption
	for _, c := range customers {
		options = append(options, filterSelectOption{Value: strconv.FormatInt(c.ID, 10), Name: c.Name, Archived: c.Archived, Selected: c.ID == page.Customer})
	}
	page.Customers = filterSelect{Action: "/quotes", Name: "customer", Label: "quotes.customer", All: "invoices.all_customers",
		Hidden: hiddenFields(quotesQuery(page.Customer, page.Status), "customer"), Groups: []filterGroup{{Options: options}}}
	if page.List.Rows, err = a.loadQuoteRows(`?1 = 0 OR q.customer_id = ?1`, page.Customer); err != nil {
		a.serverError(w, r, err)
		return
	}
	page.List.Rows = slices.DeleteFunc(page.List.Rows, func(q quoteRow) bool { return page.Status != "" && q.Status != page.Status })
	a.render(w, r, http.StatusOK, "quotes", page)
}
