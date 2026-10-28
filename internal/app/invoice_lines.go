package app

import (
	"database/sql"
	"errors"
	"net/http"
	"slices"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /invoices/{id}/lines/new", a.authed(a.lineNew))
		mux.HandleFunc("POST /invoices/{id}/lines", a.authed(a.lineCreate))
		mux.HandleFunc("GET /invoices/{id}/lines/{line}", a.authed(a.lineEdit))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}", a.authed(a.lineUpdate))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/up", a.authed(a.lineMove(-1)))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/down", a.authed(a.lineMove(+1)))
		mux.HandleFunc("POST /invoices/{id}/lines/{line}/delete", a.authed(a.lineDelete))
	})
}

// invoiceLine holds quantity and money as the decimals the user sees.
type invoiceLine struct {
	ID          int64
	Generated   bool
	Description string
	Detail      string
	Quantity    string
	Unit        string
	UnitPrice   string
	Total       string
	// Project is "" and ProjectID 0 for a line without one.
	Project   string
	ProjectID int64
}

func (l invoiceLine) project() string { return l.Project }

// lineSection is a project section; Project is "" for the lines without one, which come last.
type lineSection[L any] struct {
	Project string
	Lines   []L
}

// sections splits lines, which are in section order, into their project sections.
func sections[L any](lines []L, project func(L) string) []lineSection[L] {
	var s []lineSection[L]
	for i, l := range lines {
		if i == 0 || project(l) != project(lines[i-1]) {
			s = append(s, lineSection[L]{Project: project(l)})
		}
		s[len(s)-1].Lines = append(s[len(s)-1].Lines, l)
	}
	return s
}

// sectionOrder orders invoice_line l by project section, each placed where its first line is, then by position.
const sectionOrder = ` ORDER BY l.project_id IS NULL,
	(SELECT min(m.position) FROM invoice_line m WHERE m.invoice_id = l.invoice_id AND m.project_id = l.project_id), l.position, l.id`

// loadLines returns invoice id's lines in section order and their totals at vatPercent.
func (a *App) loadLines(id int64, unit currency.Unit, vatPercent int64) ([]invoiceLine, invoiceTotals, error) {
	lines, totals, err := queryLines(a.db, id, unit)
	return lines, totalsOf(totals, vatPercent), err
}

// queryLines returns invoice id's lines in section order and each line's total in minor units.
func queryLines(q querier, id int64, unit currency.Unit) ([]invoiceLine, []int64, error) {
	rows, err := q.Query(`SELECT l.id, l.generated, l.description, l.detail, l.quantity, l.unit, l.unit_price, ifnull(p.name, ''), ifnull(p.id, 0)
		FROM invoice_line l LEFT JOIN project p ON p.id = l.project_id WHERE l.invoice_id = ?`+sectionOrder, id)
	if err != nil {
		return nil, nil, err
	}
	defer func() { _ = rows.Close() }()
	var lines []invoiceLine
	var totals []int64
	for rows.Next() {
		var l invoiceLine
		var quantity, unitPrice int64
		if err := rows.Scan(&l.ID, &l.Generated, &l.Description, &l.Detail, &quantity, &l.Unit, &unitPrice, &l.Project, &l.ProjectID); err != nil {
			return nil, nil, err
		}
		total := lineTotal(quantity, unitPrice)
		l.Quantity, l.UnitPrice, l.Total = formatQuantity(quantity), formatAmount(unitPrice, unit), formatAmount(total, unit)
		lines = append(lines, l)
		totals = append(totals, total)
	}
	return lines, totals, rows.Err()
}

type linePage struct {
	Draft draft
	Line  invoiceLine
	// Projects are those a manual line can be assigned to.
	Projects []projectOption
	Entries  []lineEntry
	Errors   []string
}

// billsProject holds when invoice i bills project p.
const billsProject = `(i.all_projects = 1 OR p.id IN (SELECT project_id FROM invoice_project WHERE invoice_id = i.id))`

func (a *App) billedProjects(invoiceID int64) ([]projectOption, error) {
	return a.projectOptions(`EXISTS (SELECT 1 FROM invoice i WHERE i.id = ? AND i.customer_id = p.customer_id AND `+billsProject+`)`, invoiceID)
}

// renderLine shows the line form together with the entries the line claims.
func (a *App) renderLine(w http.ResponseWriter, r *http.Request, status int, p linePage) {
	entries, err := a.lineEntries(a.db, p.Draft.ID)
	if err == nil {
		p.Projects, err = a.billedProjects(p.Draft.ID)
	}
	if err != nil {
		a.serverError(w, r, err)
		return
	}
	p.Entries = entries[p.Line.ID]
	a.render(w, r, status, "invoice_line", p)
}

func (a *App) lineNew(w http.ResponseWriter, r *http.Request) {
	if d, ok := a.draftFromPath(w, r); ok {
		a.renderLine(w, r, http.StatusOK, linePage{Draft: d})
	}
}

// lineFromPath writes 404 or 500 itself when ok is false.
func (a *App) lineFromPath(w http.ResponseWriter, r *http.Request) (draft, invoiceLine, bool) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return d, invoiceLine{}, false
	}
	id, err := strconv.ParseInt(r.PathValue("line"), 10, 64)
	if err == nil {
		for _, l := range d.Lines {
			if l.ID == id {
				return d, l, true
			}
		}
	}
	a.notFound(w, r)
	return d, invoiceLine{}, false
}

func (a *App) lineEdit(w http.ResponseWriter, r *http.Request) {
	if d, l, ok := a.lineFromPath(w, r); ok {
		a.renderLine(w, r, http.StatusOK, linePage{Draft: d, Line: l})
	}
}

// lineFromForm returns the submitted line, its quantity in thousandths, its unit price
// in minor units and any validation errors as catalog keys. Its project must be one of projects.
func lineFromForm(r *http.Request, unit currency.Unit, projects []projectOption) (invoiceLine, int64, int64, []string) {
	l := invoiceLine{
		Description: strings.TrimSpace(r.PostFormValue("description")),
		Detail:      strings.TrimSpace(r.PostFormValue("detail")),
		Quantity:    strings.TrimSpace(r.PostFormValue("quantity")),
		Unit:        strings.TrimSpace(r.PostFormValue("unit")),
		UnitPrice:   strings.TrimSpace(r.PostFormValue("unit_price")),
	}
	var errs []string
	if l.Description == "" {
		errs = append(errs, "invoices.error.description")
	}
	quantity, err := parseQuantity(l.Quantity)
	if err != nil {
		errs = append(errs, "invoices.error.quantity")
	}
	unitPrice, err := parseAmount(l.UnitPrice, unit)
	if err != nil {
		errs = append(errs, "invoices.error.unit_price")
	}
	if s := r.PostFormValue("project"); s != "" {
		id, err := strconv.ParseInt(s, 10, 64)
		i := slices.IndexFunc(projects, func(p projectOption) bool { return p.ID == id })
		if err != nil || i < 0 {
			errs = append(errs, "invoices.error.line_project")
		} else {
			l.ProjectID, l.Project = id, projects[i].Name
		}
	}
	return l, quantity, unitPrice, errs
}

func (l invoiceLine) projectID() sql.NullInt64 {
	return sql.NullInt64{Int64: l.ProjectID, Valid: l.ProjectID != 0}
}

// lineForm reads the submitted line on draft d; it writes 422 or 500 itself when ok is false.
func (a *App) lineForm(w http.ResponseWriter, r *http.Request, d draft, old invoiceLine) (l invoiceLine, quantity, unitPrice int64, ok bool) {
	projects, err := a.billedProjects(d.ID)
	if err != nil {
		a.serverError(w, r, err)
		return l, 0, 0, false
	}
	l, quantity, unitPrice, errs := lineFromForm(r, d.unit, projects)
	l.ID, l.Generated = old.ID, old.Generated
	if l.Generated {
		// A generated line bills its entries' project.
		l.ProjectID, l.Project = old.ProjectID, old.Project
	}
	if errs != nil {
		a.renderLine(w, r, http.StatusUnprocessableEntity, linePage{Draft: d, Line: l, Errors: errs})
		return l, 0, 0, false
	}
	return l, quantity, unitPrice, true
}

// A new line comes last, so it follows its project's generated lines.
func (a *App) lineCreate(w http.ResponseWriter, r *http.Request) {
	d, ok := a.draftFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, ok := a.lineForm(w, r, d, invoiceLine{})
	if !ok {
		return
	}
	res, err := a.db.Exec(`INSERT INTO invoice_line (invoice_id, position, generated, description, detail, quantity, unit, unit_price, project_id)
		SELECT id, (SELECT ifnull(max(position), 0) + 1 FROM invoice_line WHERE invoice_id = ?1), 0, ?2, ?3, ?4, ?5, ?6, ?7
		FROM invoice WHERE id = ?1 AND number IS NULL`,
		d.ID, l.Description, l.Detail, quantity, l.Unit, unitPrice, l.projectID())
	a.afterInvoiceWrite(w, r, res, err, draftPath(d.ID))
}

// A line moved to another project goes last, to the end of that project's section.
func (a *App) lineUpdate(w http.ResponseWriter, r *http.Request) {
	d, old, ok := a.lineFromPath(w, r)
	if !ok {
		return
	}
	l, quantity, unitPrice, ok := a.lineForm(w, r, d, old)
	if !ok {
		return
	}
	res, err := a.db.Exec(`UPDATE invoice_line SET description = ?1, detail = ?2, quantity = ?3, unit = ?4, unit_price = ?5, project_id = ?6,
		position = CASE WHEN project_id IS ?6 THEN position ELSE (SELECT max(position) + 1 FROM invoice_line WHERE invoice_id = ?8) END
		WHERE id = ?7 AND invoice_id IN (SELECT id FROM invoice WHERE id = ?8 AND number IS NULL)`,
		l.Description, l.Detail, quantity, l.Unit, unitPrice, l.projectID(), l.ID, d.ID)
	a.afterInvoiceWrite(w, r, res, err, draftPath(d.ID))
}

// Entries claimed by the line are released by the foreign key's ON DELETE SET NULL.
func (a *App) lineDelete(w http.ResponseWriter, r *http.Request) {
	d, l, ok := a.lineFromPath(w, r)
	if !ok {
		return
	}
	res, err := a.db.Exec(`DELETE FROM invoice_line
		WHERE id = ? AND invoice_id IN (SELECT id FROM invoice WHERE id = ? AND number IS NULL)`, l.ID, d.ID)
	a.afterInvoiceWrite(w, r, res, err, draftPath(d.ID))
}

// lineMove swaps the line with its neighbour in direction step (-1 up, +1 down); at the edge it does nothing.
func (a *App) lineMove(step int) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		d, l, ok := a.lineFromPath(w, r)
		if !ok {
			return
		}
		if err := a.swapLines(d.ID, l.ID, step); errors.Is(err, sql.ErrNoRows) {
			a.notFound(w, r)
			return
		} else if err != nil {
			a.serverError(w, r, err)
			return
		}
		http.Redirect(w, r, draftPath(d.ID), http.StatusSeeOther)
	}
}

func (a *App) swapLines(invoiceID, lineID int64, step int) error {
	tx, err := a.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	var pos int64
	var project sql.NullInt64
	if err := tx.QueryRow(`SELECT l.position, l.project_id FROM invoice_line l JOIN invoice i ON i.id = l.invoice_id
		WHERE l.id = ? AND i.id = ? AND i.number IS NULL`, lineID, invoiceID).Scan(&pos, &project); err != nil {
		return err
	}
	// The neighbour is in the same project section, so sections keep their order.
	neighbour := `SELECT id, position FROM invoice_line WHERE invoice_id = ? AND project_id IS ? AND position > ? ORDER BY position LIMIT 1`
	if step < 0 {
		neighbour = `SELECT id, position FROM invoice_line WHERE invoice_id = ? AND project_id IS ? AND position < ? ORDER BY position DESC LIMIT 1`
	}
	var otherID, otherPos int64
	err = tx.QueryRow(neighbour, invoiceID, project, pos).Scan(&otherID, &otherPos)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE invoice_line SET position = ? WHERE id = ?`, otherPos, lineID); err != nil {
		return err
	}
	if _, err := tx.Exec(`UPDATE invoice_line SET position = ? WHERE id = ?`, pos, otherID); err != nil {
		return err
	}
	return tx.Commit()
}
