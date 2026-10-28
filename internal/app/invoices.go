package app

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"golang.org/x/text/currency"

	"github.com/da-wilky/compact-company/internal/config"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices", a.authed(a.invoiceList))
		mux.HandleFunc("GET /invoices/new", a.authed(a.draftNew))
		mux.HandleFunc("POST /invoices", a.authed(a.draftCreate))
		mux.HandleFunc("POST /invoices/delete", a.authed(a.draftDeleteSelected))
		mux.HandleFunc("GET /invoices/{id}", a.authed(a.invoiceShow))
		mux.HandleFunc("POST /invoices/{id}", a.authed(a.draftUpdate))
		mux.HandleFunc("POST /invoices/{id}/delete", a.authed(a.draftDelete))
	})
}

var groupings = []string{"project", "entry"}

// Invoice kinds, as the invoice table's CHECK constraint allows them.
const (
	kindInvoice      = "invoice"
	kindCancellation = "cancellation"
)

// draftVATPercent is the business's current VAT rate, which drafts are totalled at.
func (a *App) draftVATPercent() (int64, error) {
	b, err := a.loadBusiness()
	if err != nil || b.TaxMode != config.TaxModeStandard {
		return 0, err
	}
	return int64(b.VATRate), nil
}

type invoiceRow struct {
	ID        int64
	Number    string
	Customer  string
	IssueDate string
	PaidDate  string
	Gross     string
	Currency  string
	Status    string
}

// Selectable reports whether r may be selected for a mass action: only drafts can be deleted.
func (r invoiceRow) Selectable() bool {
	return r.Status == "draft"
}

type invoicesPage struct {
	Rows      []invoiceRow
	Customers []customerOption
	Customer  int64
	Status    string
	Statuses  []string
	DeleteURL string
	selectionRefusal[invoiceRow]
}

type customerOption struct {
	ID   int64
	Name string
}

var invoiceStatuses = []string{"draft", "issued", "paid", "cancelled"}

// StatusLink is the list URL for status, keeping the customer filter.
func (p invoicesPage) StatusLink(status string) string {
	q := url.Values{}
	if p.Customer != 0 {
		q.Set("customer", strconv.FormatInt(p.Customer, 10))
	}
	if status != "" {
		q.Set("status", status)
	}
	if len(q) == 0 {
		return "/invoices"
	}
	return "/invoices?" + q.Encode()
}

func (a *App) invoiceList(w http.ResponseWriter, r *http.Request) {
	a.listInvoices(w, r, http.StatusOK, selectionRefusal[invoiceRow]{})
}

// listInvoices renders the listing r asks for, with the selection a refused mass action leaves behind.
func (a *App) listInvoices(w http.ResponseWriter, r *http.Request, status int, refused selectionRefusal[invoiceRow]) {
	page := invoicesPage{Statuses: invoiceStatuses, selectionRefusal: refused}
	page.Customer, _ = strconv.ParseInt(r.FormValue("customer"), 10, 64)
	if s := r.FormValue("status"); slices.Contains(invoiceStatuses, s) {
		page.Status = s
	}
	page.DeleteURL = "/invoices/delete" + strings.TrimPrefix(page.StatusLink(page.Status), "/invoices")
	var err error
	if page.Customers, err = a.customerOptions(); err != nil {
		a.serverError(w, r, err)
		return
	}
	// Issued rows show their snapshot, so later customer or business changes don't reach them.
	rows, err := a.db.Query(`SELECT * FROM (SELECT i.id, ifnull(i.number, '') number, ifnull(i.snapshot ->> '$.customer.name', c.name),
		ifnull(i.issue_date, '') issue_date, ifnull(i.paid_date, '') paid_date, i.currency, i.gross,
		`+invoiceStatus+` status
		FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE (?1 = 0 OR i.customer_id = ?1))
		WHERE ?2 = '' OR ?2 = status OR (?2 = 'cancelled' AND status = 'partially_cancelled') OR (?2 = 'paid' AND paid_date != '')
		ORDER BY number != '', issue_date DESC, id DESC`, page.Customer, page.Status)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var row invoiceRow
		var gross sql.NullInt64
		if err := rows.Scan(&row.ID, &row.Number, &row.Customer, &row.IssueDate, &row.PaidDate, &row.Currency, &gross, &row.Status); err != nil {
			a.serverError(w, r, err)
			return
		}
		if gross.Valid {
			row.Gross = formatAmount(gross.Int64, currency.MustParseISO(row.Currency))
		}
		page.Rows = append(page.Rows, row)
	}
	if err := rows.Err(); err != nil {
		a.serverError(w, r, err)
		return
	}
	vat, err := a.draftVATPercent()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	for i := range page.Rows {
		row := &page.Rows[i]
		if row.Number != "" {
			continue
		}
		unit := currency.MustParseISO(row.Currency)
		_, totals, err := a.loadLines(row.ID, unit, vat)
		if err != nil {
			a.serverError(w, r, err)
			return
		}
		row.Gross = formatAmount(totals.Gross, unit)
	}
	a.render(w, r, status, "invoices", page)
}

// customerOptions lists every customer, archived ones included: finished work can still be billed.
func (a *App) customerOptions() ([]customerOption, error) {
	rows, err := a.db.Query(`SELECT id, name FROM customer ORDER BY name, id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var opts []customerOption
	for rows.Next() {
		var o customerOption
		if err := rows.Scan(&o.ID, &o.Name); err != nil {
			return nil, err
		}
		opts = append(opts, o)
	}
	return opts, rows.Err()
}

type draftNewForm struct {
	Customer        string
	From, To        string
	Grouping        string
	RoundingMinutes string
	PaymentDays     string
	AllProjects     bool
	Projects        []string
}

func (f draftNewForm) Picked(id int64) bool {
	return slices.Contains(f.Projects, strconv.FormatInt(id, 10))
}

type draftNewPage struct {
	Form       draftNewForm
	Customers  []customerOption
	Projects   []projectOption
	Groupings  []string
	Increments []int
	Errors     []string
}

type projectOption struct {
	ID       int64
	Name     string
	Archived bool
}

// projectOptions lists the projects p meeting cond, archived ones included: their finished work can still be billed.
func (a *App) projectOptions(cond string, arg any) ([]projectOption, error) {
	rows, err := a.db.Query(`SELECT p.id, p.name, p.archived FROM project p WHERE `+cond+` ORDER BY p.name, p.id`, arg)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var opts []projectOption
	for rows.Next() {
		var o projectOption
		if err := rows.Scan(&o.ID, &o.Name, &o.Archived); err != nil {
			return nil, err
		}
		opts = append(opts, o)
	}
	return opts, rows.Err()
}

func (a *App) draftNew(w http.ResponseWriter, r *http.Request) {
	loc, err := a.viewerLocation(r)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	now := a.now().In(loc)
	thisMonth := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, time.UTC)
	// The form selects only a customer it lists, so any other value selects nothing.
	a.renderDraftNew(w, r, http.StatusOK, draftNewForm{
		Customer:        r.FormValue("customer"),
		From:            thisMonth.AddDate(0, -1, 0).Format(time.DateOnly),
		To:              thisMonth.AddDate(0, 0, -1).Format(time.DateOnly),
		Grouping:        "project",
		RoundingMinutes: strconv.Itoa(a.cfg.Invoice.RoundingMinutes),
		PaymentDays:     strconv.Itoa(a.cfg.Invoice.PaymentDays),
		AllProjects:     true,
	}, nil)
}

func (a *App) renderDraftNew(w http.ResponseWriter, r *http.Request, status int, f draftNewForm, errs []string) {
	customers, err := a.customerOptions()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	// The browser selects the first customer when none of them matches, so its projects are the ones to offer.
	if len(customers) > 0 && !slices.ContainsFunc(customers, func(c customerOption) bool { return strconv.FormatInt(c.ID, 10) == f.Customer }) {
		f.Customer = strconv.FormatInt(customers[0].ID, 10)
	}
	projects, err := a.projectOptions("p.customer_id = ?", f.Customer)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	a.render(w, r, status, "invoice_new", draftNewPage{Form: f, Customers: customers, Projects: projects, Groupings: groupings, Increments: config.RoundingIncrements, Errors: errs})
}

func validOptionalRange(from, to string) bool {
	if from == "" && to == "" {
		return true
	}
	f, errF := time.Parse(time.DateOnly, from)
	t, errT := time.Parse(time.DateOnly, to)
	return errF == nil && errT == nil && !t.Before(f)
}

func parsePaymentDays(s string) (int, bool) {
	n, err := strconv.Atoi(strings.TrimSpace(s))
	return n, err == nil && n > 0
}

func (a *App) draftCreate(w http.ResponseWriter, r *http.Request) {
	f := draftNewForm{
		Customer:        r.PostFormValue("customer"),
		From:            strings.TrimSpace(r.PostFormValue("from")),
		To:              strings.TrimSpace(r.PostFormValue("to")),
		Grouping:        r.PostFormValue("grouping"),
		RoundingMinutes: r.PostFormValue("rounding_minutes"),
		PaymentDays:     strings.TrimSpace(r.PostFormValue("payment_days")),
		AllProjects:     r.PostFormValue("all_projects") != "",
	}
	// The project checkboxes are only hidden while "All projects" is ticked, so they still submit.
	if !f.AllProjects {
		f.Projects = r.PostForm["project"]
	}
	var errs []string
	customerID, err := strconv.ParseInt(f.Customer, 10, 64)
	if err != nil {
		errs = append(errs, "invoices.error.customer")
	}
	if !validOptionalRange(f.From, f.To) {
		errs = append(errs, "invoices.error.range")
	}
	if !slices.Contains(groupings, f.Grouping) {
		errs = append(errs, "invoices.error.grouping")
	}
	rounding, err := strconv.Atoi(f.RoundingMinutes)
	if err != nil || !slices.Contains(config.RoundingIncrements, rounding) {
		errs = append(errs, "invoices.error.rounding")
	}
	paymentDays, ok := parsePaymentDays(f.PaymentDays)
	if !ok {
		errs = append(errs, "invoices.error.payment_days")
	}
	projectIDs, ok := parseIDs(f.Projects)
	if !ok || (!f.AllProjects && len(projectIDs) == 0) {
		errs = append(errs, "invoices.error.projects")
	}
	if errs != nil {
		a.renderDraftNew(w, r, http.StatusUnprocessableEntity, f, errs)
		return
	}
	idsJSON, err := json.Marshal(projectIDs)
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	tx, err := a.db.Begin()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	defer func() { _ = tx.Rollback() }()
	var id int64
	err = tx.QueryRow(`INSERT INTO invoice
		(kind, customer_id, range_from, range_to, service_from, service_to, grouping, rounding_minutes, payment_days, currency, all_projects)
		SELECT ?7, id, ?1, ?2, '', '', ?3, ?4, ?5, currency, ?8 FROM customer WHERE id = ?6 RETURNING id`,
		f.From, f.To, f.Grouping, rounding, paymentDays, customerID, kindInvoice, f.AllProjects).Scan(&id)
	if errors.Is(err, sql.ErrNoRows) {
		a.renderDraftNew(w, r, http.StatusUnprocessableEntity, f, []string{"invoices.error.customer"})
		return
	}
	if err == nil && len(projectIDs) > 0 {
		var res sql.Result
		var n int64
		res, err = tx.Exec(`INSERT INTO invoice_project (invoice_id, project_id)
			SELECT ?, id FROM project WHERE customer_id = ? AND id IN (SELECT value FROM json_each(?))`, id, customerID, string(idsJSON))
		if err == nil {
			n, err = res.RowsAffected()
		}
		if err == nil && n < int64(len(projectIDs)) {
			a.renderDraftNew(w, r, http.StatusUnprocessableEntity, f, []string{"invoices.error.projects"})
			return
		}
	}
	if err == nil {
		err = a.pullEntries(tx, id)
	}
	if err == nil {
		err = tx.Commit()
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	http.Redirect(w, r, draftPath(id), http.StatusSeeOther)
}

func draftPath(id int64) string {
	return "/invoices/" + strconv.FormatInt(id, 10)
}

// draftHeader is what stays editable on a draft after creation.
type draftHeader struct {
	From, To               string
	ServiceFrom, ServiceTo string
	PaymentDays            string
}

type draft struct {
	ID              int64
	CustomerNumber  string
	CustomerName    string
	Currency        string
	Grouping        string
	RoundingMinutes int
	AllProjects     bool
	// Projects names the fixed choice of projects the draft bills, unless AllProjects.
	Projects []string
	Header   draftHeader
	// ServicePeriod is set while the draft claims entries, which then decide it.
	ServicePeriod   *period
	Lines           []invoiceLine
	Net, VAT, Gross string
	VATPercent      int64
	unit            currency.Unit
}

func (d draft) Sections() []lineSection[invoiceLine] { return sections(d.Lines, invoiceLine.project) }

type draftPage struct {
	Draft  draft
	Errors []string
	renderFailure
}

// loadDraft returns sql.ErrNoRows unless id is a draft.
func (a *App) loadDraft(id int64) (draft, error) {
	d := draft{ID: id}
	var paymentDays int
	err := a.db.QueryRow(`SELECT c.number, c.name, i.currency, i.grouping, i.rounding_minutes,
		i.range_from, i.range_to, i.service_from, i.service_to, i.payment_days, i.all_projects
		FROM invoice i JOIN customer c ON c.id = i.customer_id
		WHERE i.id = ? AND i.kind = ? AND i.number IS NULL`, id, kindInvoice).
		Scan(&d.CustomerNumber, &d.CustomerName, &d.Currency, &d.Grouping, &d.RoundingMinutes,
			&d.Header.From, &d.Header.To, &d.Header.ServiceFrom, &d.Header.ServiceTo, &paymentDays, &d.AllProjects)
	if err != nil {
		return d, err
	}
	if d.Projects, err = a.draftProjects(id); err != nil {
		return d, err
	}
	d.Header.PaymentDays = strconv.Itoa(paymentDays)
	if p, ok, err := entryPeriod(a.db, id); err != nil {
		return d, err
	} else if ok {
		d.ServicePeriod = &p
	}
	d.unit = currency.MustParseISO(d.Currency)
	if d.VATPercent, err = a.draftVATPercent(); err != nil {
		return d, err
	}
	lines, totals, err := a.loadLines(id, d.unit, d.VATPercent)
	if err != nil {
		return d, err
	}
	d.Lines = lines
	d.Net, d.VAT, d.Gross = formatAmount(totals.Net, d.unit), formatAmount(totals.VAT, d.unit), formatAmount(totals.Gross, d.unit)
	return d, nil
}

func (a *App) draftProjects(id int64) ([]string, error) {
	rows, err := a.db.Query(`SELECT p.name FROM invoice_project ip JOIN project p ON p.id = ip.project_id
		WHERE ip.invoice_id = ? ORDER BY p.name, p.id`, id)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var names []string
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, err
		}
		names = append(names, name)
	}
	return names, rows.Err()
}

// draftFromPath writes 404 or 500 itself when ok is false.
func (a *App) draftFromPath(w http.ResponseWriter, r *http.Request) (draft, bool) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return draft{}, false
	}
	d, err := a.loadDraft(id)
	if errors.Is(err, sql.ErrNoRows) {
		a.notFound(w, r)
		return d, false
	}
	if err != nil {
		a.serverError(w, r, err)
		return d, false
	}
	return d, true
}

func (a *App) draftUpdate(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	d.Header.From = strings.TrimSpace(r.PostFormValue("from"))
	d.Header.To = strings.TrimSpace(r.PostFormValue("to"))
	d.Header.PaymentDays = strings.TrimSpace(r.PostFormValue("payment_days"))
	if d.ServicePeriod == nil {
		d.Header.ServiceFrom = strings.TrimSpace(r.PostFormValue("service_from"))
		d.Header.ServiceTo = strings.TrimSpace(r.PostFormValue("service_to"))
	}
	var errs []string
	if !validOptionalRange(d.Header.From, d.Header.To) {
		errs = append(errs, "invoices.error.range")
	}
	if !validOptionalRange(d.Header.ServiceFrom, d.Header.ServiceTo) {
		errs = append(errs, "invoices.error.service_period")
	}
	paymentDays, ok := parsePaymentDays(d.Header.PaymentDays)
	if !ok {
		errs = append(errs, "invoices.error.payment_days")
	}
	if errs != nil {
		a.render(w, r, http.StatusUnprocessableEntity, "invoice", draftPage{Draft: d, Errors: errs})
		return
	}
	res, err := a.db.Exec(`UPDATE invoice SET range_from = ?, range_to = ?, service_from = ?, service_to = ?, payment_days = ?
		WHERE id = ? AND number IS NULL`,
		d.Header.From, d.Header.To, d.Header.ServiceFrom, d.Header.ServiceTo, paymentDays, d.ID)
	a.afterInvoiceWrite(w, r, res, err, draftPath(d.ID))
}

// Deleting the draft cascades to its lines, which releases their entries.
func (a *App) draftDelete(w http.ResponseWriter, r *http.Request) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		a.notFound(w, r)
		return
	}
	res, err := deleteDraft(a.db, id)
	a.afterInvoiceWrite(w, r, res, err, "/invoices")
}

// deleteDraft deletes invoice id if it is a draft; cascades drop its lines and chosen projects and release its entries.
func deleteDraft(db execer, id int64) (sql.Result, error) {
	return db.Exec(`DELETE FROM invoice WHERE id = ? AND kind = ? AND number IS NULL`, id, kindInvoice)
}

// draftDeleteSelected deletes all selected drafts, or none when any of them is not a draft,
// and answers with the listing its query names.
func (a *App) draftDeleteSelected(w http.ResponseWriter, r *http.Request) {
	massWrite(a, w, r, massAction[invoiceRow]{
		load: func(tx *sql.Tx, ids []int64) ([]invoiceRow, error) {
			in, args := inList(ids)
			rows, err := tx.Query(`SELECT i.id, ifnull(i.number, ''), ifnull(i.snapshot ->> '$.customer.name', c.name), `+invoiceStatus+`
				FROM invoice i JOIN customer c ON c.id = i.customer_id
				WHERE i.id`+in+` ORDER BY i.number, i.id`, args...)
			if err != nil {
				return nil, err
			}
			defer func() { _ = rows.Close() }()
			var found []invoiceRow
			for rows.Next() {
				var row invoiceRow
				if err := rows.Scan(&row.ID, &row.Number, &row.Customer, &row.Status); err != nil {
					return nil, err
				}
				found = append(found, row)
			}
			return found, rows.Err()
		},
		blocks:  func(_ *sql.Tx, row *invoiceRow) (bool, error) { return !row.Selectable(), nil },
		blocked: "invoices.error.delete_blocked",
		apply: func(tx *sql.Tx, ids []int64) (string, error) {
			for _, id := range ids {
				if _, err := deleteDraft(tx, id); err != nil {
					return "", err
				}
			}
			return "", nil
		},
		list: func(status int, s selectionRefusal[invoiceRow]) { a.listInvoices(w, r, status, s) },
	})
}

// afterInvoiceWrite redirects to done, or answers 404 when the write matched no row
// because the invoice (or line) is gone or no longer in the state the write requires.
func (a *App) afterInvoiceWrite(w http.ResponseWriter, r *http.Request, res sql.Result, err error, done string) {
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	n, err := res.RowsAffected()
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	if n == 0 {
		a.notFound(w, r)
		return
	}
	http.Redirect(w, r, done, http.StatusSeeOther)
}
