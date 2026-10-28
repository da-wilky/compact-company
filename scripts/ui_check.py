"""Render every page in English and German at desktop and phone width in light and dark, fail on layout breakage, save screenshots.

Usage: ui_check.py <base-url> <database> <screenshot-dir> <chromium> [path…], against a fresh dev-mode instance (it seeds data).
Paths render only those pages; the seed is deterministic, so e.g. /invoices/1 is the draft with lines, /invoices/4 the empty one without a range
and /invoices/5 to /invoices/8 open invoices, the even ones overdue.
"""

import datetime
import re
import sqlite3
import sys

from playwright.sync_api import sync_playwright

base, database, out, chromium = sys.argv[1:5]
only = sys.argv[5:]

VIEWPORTS = {"desktop": (1280, 800), "phone": (390, 800)}

# Elements whose text is font-size 0 are invisible; icon buttons that draw a ::before keep their text as the accessible name.
FIND_BREAKAGE = """() => {
  const problems = [];
  if (document.documentElement.scrollWidth > innerWidth + 1)
    problems.push(`horizontal overflow: ${document.documentElement.scrollWidth}px > ${innerWidth}px`);
  for (const el of document.body.querySelectorAll("*")) {
    const text = [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join("");
    if (!text || !el.getClientRects().length) continue;
    if (getComputedStyle(el).fontSize === "0px" && getComputedStyle(el, "::before").content === "none")
      problems.push(`invisible text (font-size 0): ${JSON.stringify(text.slice(0, 40))}`);
  }
  return problems;
}"""


def seed(api):
    today = datetime.date.today()
    monday = today - datetime.timedelta(days=today.weekday())
    api.post(f"{base}/customers", form={"name": "Aufwind eG", "address": "Hauptstr. 1\n12345 Berlin", "email": "billing@aufwind.example", "currency": "EUR", "invoice_language": "de", "timezone": "Europe/Berlin", "hourly_rate": "85"})
    customer = re.search(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text()).group(1)
    for name in ("Website relaunch", "Hosting"):
        api.post(f"{base}/customers/{customer}/projects", form={"name": name, "hourly_rate": "", "billable": "on"})
    projects = dict((n, i) for i, n in re.findall(r'href="/projects/(\d+)">([^<]+)<', api.get(f"{base}/customers/{customer}/projects").text()))
    for day, hours, project in [(0, (8, 12), "Website relaunch"), (1, (9, 11), "Hosting"), (2, (13, 17), "Website relaunch"), (3, (8, 9), "")]:
        d = monday + datetime.timedelta(days=day)
        api.post(f"{base}/entries", form={"project": projects.get(project, ""), "start": f"{d}T{hours[0]:02}:00", "end": f"{d}T{hours[1]:02}:00", "description": f"{project or 'Admin'} work", "billable": ""})
    api.post(f"{base}/timers", form={"project": projects["Hosting"], "description": "Server update"})
    draft = api.post(f"{base}/invoices", form={"customer": customer, "from": str(monday), "to": str(today), "grouping": "project", "rounding_minutes": "15", "payment_days": "14"}, max_redirects=0).headers["location"]
    for line in [("Website relaunch", "Design and implementation of the new landing page", "12.5", "h", "85"), ("Hosting", "", "1", "month", "29.90")]:
        api.post(f"{base}{draft}/lines", form=dict(zip(("description", "detail", "quantity", "unit", "unit_price"), line)))
    line = re.search(rf'href="({draft}/lines/\d+)"', api.get(f"{base}{draft}").text()).group(1)
    entry = re.findall(r'href="(/entries/\d+)\?', api.get(f"{base}/entries").text())[-1]  # Monday's, claimed by the draft
    issued = api.post(f"{base}/invoices", form={"customer": customer, "from": str(monday), "to": str(today), "grouping": "project", "rounding_minutes": "0", "payment_days": "14"}, max_redirects=0).headers["location"]
    api.post(f"{base}{issued}/lines", form={"description": "Workshop", "detail": "On site, two days", "quantity": "16", "unit": "h", "unit_price": "85"})
    business = {"name": "Samuel Wilk", "company": "swilk", "address": "Hauptstraße 1\n12345 Berlin", "tax_number": "12/345/67890", "tax_mode": "standard", "vat_rate": "19"}
    api.post(f"{base}/settings/business", form=business)
    api.post(f"{base}{issued}/lines", form={"description": "Travel", "detail": "", "quantity": "1", "unit": "flat", "unit_price": "120"})
    api.post(f"{base}{issued}/issue")
    travel = re.search(r'name="line" value="(\d+)"> Travel', api.get(f"{base}{issued}/cancel").text()).group(1)
    cancellation = api.post(f"{base}{issued}/cancel", form={"line": travel, "reason": "Travel was included in the workshop price"}, max_redirects=0).headers["location"]
    api.post(f"{base}{issued}/paid", form={"paid_date": str(today)})
    # Unset again so the draft's preview shows the missing-data page instead of a PDF download.
    api.post(f"{base}/settings/business", form=business | {"name": ""})
    empty = api.post(f"{base}/invoices", form={"customer": customer, "from": "", "to": "", "grouping": "project", "rounding_minutes": "0", "payment_days": "14"}, max_redirects=0).headers["location"]
    seed_open_invoices(api, customer, business)
    # Time after the drafts were made, so the overview shows unbilled time both on a draft and on no invoice.
    last_friday = monday - datetime.timedelta(days=3)
    api.post(f"{base}/entries", form={"project": projects["Website relaunch"], "start": f"{last_friday}T09:00", "end": f"{last_friday}T12:30", "description": "Content migration", "billable": ""})
    api.post(f"{base}/customers", form={"name": "Nordlicht Logistik GmbH", "address": "Am Hafen 5\n20457 Hamburg", "email": "rechnung@nordlicht.example", "currency": "EUR", "invoice_language": "de", "timezone": "Europe/Berlin", "hourly_rate": "90"})
    other = max(map(int, re.findall(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text())))
    api.post(f"{base}/customers/{other}/projects", form={"name": "Fleet dashboard", "hourly_rate": "", "billable": "on"})
    fleet = re.search(r'href="/projects/(\d+)">', api.get(f"{base}/customers/{other}/projects").text()).group(1)
    api.post(f"{base}/entries", form={"project": fleet, "start": f"{last_friday}T13:30", "end": f"{last_friday}T15:00", "description": "Dashboard review", "billable": ""})
    return customer, projects["Website relaunch"], draft, line, entry, issued, cancellation, empty


def seed_open_invoices(api, customer, business):
    api.post(f"{base}/customers", form={"name": "Northwind Trading Ltd", "address": "1 Harbour St\nPortland, ME 04101", "email": "ap@northwind.example", "currency": "USD", "invoice_language": "en", "timezone": "America/New_York", "hourly_rate": "120"})
    northwind = re.findall(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text())[-1]
    api.post(f"{base}/settings/business", form=business)
    today = datetime.date.today()
    for owner, description, quantity, price, overdue_days in [(customer, "Maintenance", "6", "85", 0), (customer, "Consulting", "3.5", "85", 12),
                                                              (northwind, "Integration", "10", "120", 0), (northwind, "Support retainer", "1", "450", 4)]:
        invoice = api.post(f"{base}/invoices", form={"customer": owner, "from": str(today), "to": str(today), "grouping": "project", "rounding_minutes": "0", "payment_days": "14"}, max_redirects=0).headers["location"]
        api.post(f"{base}{invoice}/lines", form={"description": description, "detail": "", "quantity": quantity, "unit": "h", "unit_price": price})
        api.post(f"{base}{invoice}/issue")
        if overdue_days:
            # Issuing always dates today; only the database can make an invoice overdue now.
            due = today - datetime.timedelta(days=overdue_days)
            with sqlite3.connect(database) as db:
                db.execute("UPDATE invoice SET issue_date = ?, due_date = ? WHERE id = ?", (str(due - datetime.timedelta(days=14)), str(due), invoice.rsplit("/", 1)[1]))
    api.post(f"{base}/settings/business", form=business | {"name": ""})


def main():
    failures = []
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=chromium)
        api = browser.new_context().request
        customer, project, draft, line, entry, issued, cancellation, empty = seed(api)
        today = datetime.date.today().isoformat()
        pages = only or ["/", "/week", "/week?everyone=1", "/entries", f"/entries?day={today}&project={project}", "/entries/new", entry,
                 "/customers", "/customers/new", f"/customers/{customer}", f"/customers/{customer}/projects", f"/projects/{project}",
                 "/invoices", "/invoices/new", draft, f"{draft}/preview", f"{draft}/lines/new", line, f"{draft}/refresh",
                 f"{draft}/issue", issued, f"{issued}/cancel", cancellation, empty, f"{empty}/refresh",
                 "/settings", "/profile", "/no-such-page"]
        for lang in ("en", "de"):
            page = browser.new_page()
            page.goto(f"{base}/profile")
            page.select_option("select[name=language]", lang)
            page.click("form.settings button")
            page.close()
            for viewport, (width, height) in VIEWPORTS.items():
                for scheme in ("light", "dark"):
                    page = browser.new_page(viewport={"width": width, "height": height}, color_scheme=scheme)
                    for path in pages:
                        if (status := page.goto(base + path).status) >= 500:
                            failures.append(f"{lang} {viewport} {scheme} {path}: HTTP {status}")
                        name = re.sub(r"[^a-z0-9]+", "-", path.lower()).strip("-") or "home"
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-{name}.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} {path}: {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if not only:
                        page.goto(base + "/week")
                        page.click(".picker-trigger")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-week-picker.png")
                        failures += [f"{lang} {viewport} {scheme} /week (picker open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.goto(base + draft)
                        page.click(f'form[action="{draft}/delete"] button')
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-confirm-dialog.png")
                        failures += [f"{lang} {viewport} {scheme} {draft} (confirm dialog open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if "/entries" in pages:
                        page.goto(base + "/entries")
                        page.check("form.selection [data-select-all]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-entries-selected.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /entries (rows selected): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click(".selection-actions button")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-entries-selected-confirm.png")
                        failures += [f"{lang} {viewport} {scheme} /entries (delete confirm open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    page.close()
        browser.close()
    print(f"screenshots: {out}")
    for f in failures:
        print(f"FAIL {f}")
    sys.exit(1 if failures else 0)


main()
