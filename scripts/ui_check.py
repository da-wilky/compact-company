"""Render every page in English and German at desktop and phone width in light and dark, fail on layout breakage, save screenshots.

Tabbing through each page (English, light) fails on any element that takes focus without an outline or box-shadow.

Usage: ui_check.py <base-url> <database> <screenshot-dir> <chromium> [path… | --changed <git-ref>], against a fresh dev-mode instance (it seeds data).
Paths render only those pages; the seed is deterministic, so e.g. /invoices/1 is the draft with lines, /invoices/4 the empty one without a range
and /invoices/5 to /invoices/8 open invoices, the even ones overdue; /todos/1 is an overdue todo; /quotes/1 is the draft quote with
lines and sections, /quotes/2 the empty one. --changed renders only the pages whose template differs from <git-ref>.
Interactions select by route, data-* or ARIA attributes, so renaming a styling class can't break them.
"""

import datetime
import re
import sqlite3
import subprocess
import sys

from playwright.sync_api import sync_playwright

base, database, out, chromium = sys.argv[1:5]
only = sys.argv[5:]
changed = None
if only[:1] == ["--changed"]:
    files = subprocess.run(["git", "diff", "--name-only", only[1], "--", "internal/app/templates", "internal/app/static", "internal/app/messages"],
                           capture_output=True, text=True, check=True).stdout.split()
    names = [m[1] for f in files if (m := re.fullmatch(r"internal/app/templates/(\w+)\.html", f)) and m[1] != "layout"]
    # Partials, the layout, CSS, JS and strings reach every page, so any of those renders them all.
    changed = set(names) if len(names) == len(files) else None
    only = []

VIEWPORTS = {"desktop": (1280, 800), "phone": (390, 800)}

# Elements whose text is font-size 0 are invisible; icon buttons that draw a ::before keep their text as the accessible name.
# Text is cut off when it is wider than its box (unless the box shows an ellipsis or scrolls), or when it sits past the
# viewport's edge outside a horizontal scroller: fixed elements, like the phone tab bar, don't widen the page.
FIND_BREAKAGE = """() => {
  const problems = [];
  const scrolls = (el) => ["auto", "scroll"].includes(getComputedStyle(el).overflowX);
  const inScroller = (el) => { for (let a = el.parentElement; a; a = a.parentElement) if (scrolls(a) && a.scrollWidth > a.clientWidth) return true; return false; };
  if (document.documentElement.scrollWidth > innerWidth + 1)
    problems.push(`horizontal overflow: ${document.documentElement.scrollWidth}px > ${innerWidth}px`);
  for (const el of document.body.querySelectorAll("*")) {
    const text = [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join("");
    if (!text || !el.getClientRects().length) continue;
    const style = getComputedStyle(el);
    if (style.fontSize === "0px" && getComputedStyle(el, "::before").content === "none")
      problems.push(`invisible text (font-size 0): ${JSON.stringify(text.slice(0, 40))}`);
    const rect = el.getBoundingClientRect();
    if ((style.display !== "inline" && el.scrollWidth > el.clientWidth + 1 && style.textOverflow !== "ellipsis" && !scrolls(el))
        || ((rect.right > innerWidth + 1 || rect.left < -1) && !inScroller(el)))
      problems.push(`cut-off text: ${JSON.stringify(text.slice(0, 40))}`);
  }
  return problems;
}"""

# The element Tab just focused, and whether it shows focus (an outline or a box-shadow); null once focus leaves the page.
FOCUSED = """() => {
  const el = document.activeElement;
  if (!el || el === document.body) return null;
  const s = getComputedStyle(el);
  const name = el.getAttribute("aria-label") || el.textContent.trim().slice(0, 30) || el.getAttribute("name") || "";
  return {index: [...document.querySelectorAll("*")].indexOf(el), shown: (s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== "none",
          name: `${el.tagName.toLowerCase()} ${JSON.stringify(name)}`};
}"""


def unfocusable(page):
    """Tabs through the page and names every element that takes focus without showing it."""
    problems, seen = [], set()
    while (el := (page.keyboard.press("Tab"), page.evaluate(FOCUSED))[1]) and el["index"] not in seen:
        seen.add(el["index"])
        if not el["shown"]:
            problems.append(f"no visible focus: {el['name']}")
    return problems


# Selects the first row the list doesn't offer for selection, as a stale page would post it, so the action is refused.
SELECT_UNSELECTABLE = """([form, link]) => {
  const row = [...document.querySelectorAll("tbody tr")].find((tr) => !tr.querySelector("input[name=id]") && tr.querySelector(`a[href^="${link}"]`));
  const box = Object.assign(document.createElement("input"), {type: "checkbox", name: "id", hidden: true, checked: true});
  box.value = row.querySelector(`a[href^="${link}"]`).getAttribute("href").slice(link.length).match(/^\\d+/)[0];
  box.setAttribute("form", form);
  document.body.append(box);
}"""


def seed(api):
    today = datetime.date.today()
    monday = today - datetime.timedelta(days=today.weekday())
    api.post(f"{base}/customers", form={"name": "Aufwind eG", "address": "Hauptstr. 1\n12345 Berlin", "email": "billing@aufwind.example", "currency": "EUR", "invoice_language": "de", "timezone": "Europe/Berlin", "hourly_rate": "85"})
    customer = re.search(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text()).group(1)
    for name in ("Website relaunch", "Hosting"):
        api.post(f"{base}/customers/{customer}/projects", form={"name": name, "hourly_rate": "", "billable": "on"})
    projects = dict((n, i) for i, n in re.findall(r'href="/projects/(\d+)">([^<]+)<', api.get(f"{base}/customers/{customer}/projects").text()))
    for title, assignee, priority, due in [("Write the landing page copy for the relaunch and its pricing section", "1", "2", today - datetime.timedelta(days=1)),
                                           ("Set up redirects", "", "1", today + datetime.timedelta(days=5)), ("Pick fonts", "1", "0", ""), ("Review analytics", "", "1", "")]:
        api.post(f"{base}/projects/{projects['Website relaunch']}/todos", form={"title": title, "description": "", "assignee": assignee, "priority": priority, "due_date": str(due)})
    for todo, status in [(3, 2), (4, 4)]:
        api.post(f"{base}/todos/{todo}/status", form={"status": str(status)})
    for day, hours, project in [(0, (8, 12), "Website relaunch"), (1, (9, 11), "Hosting"), (2, (13, 17), "Website relaunch"), (3, (8, 9), "")]:
        d = monday + datetime.timedelta(days=day)
        api.post(f"{base}/entries", form={"project": projects.get(project, ""), "start": f"{d}T{hours[0]:02}:00", "end": f"{d}T{hours[1]:02}:00", "description": f"{project or 'Admin'} work", "billable": ""})
    api.post(f"{base}/timers", form={"project": "todo:1", "description": "Landing page"})
    draft = api.post(f"{base}/invoices", form={"customer": customer, "from": str(monday), "to": str(today), "grouping": "project", "rounding_minutes": "15", "payment_days": "14", "all_projects": "1"}, max_redirects=0).headers["location"]
    for line in [("Website relaunch", "Design and implementation of the new landing page", "12.5", "h", "85", ""), ("Hosting", "", "1", "month", "29.90", projects["Hosting"])]:
        api.post(f"{base}{draft}/lines", form=dict(zip(("description", "detail", "quantity", "unit", "unit_price", "project"), line)))
    line = re.search(rf'href="({draft}/lines/\d+)"', api.get(f"{base}{draft}").text()).group(1)
    entry = re.findall(r'href="(/entries/\d+)\?', api.get(f"{base}/entries").text())[-1]  # Monday's, claimed by the draft
    # Last week's time, so the issued invoice has project sections next to its free lines.
    for day, project in [(-7, "Website relaunch"), (-6, "Hosting")]:
        d = monday + datetime.timedelta(days=day)
        api.post(f"{base}/entries", form={"project": projects[project], "start": f"{d}T09:00", "end": f"{d}T11:00", "description": f"{project} work", "billable": ""})
    issued = api.post(f"{base}/invoices", form={"customer": customer, "from": str(monday - datetime.timedelta(days=7)), "to": str(today), "grouping": "project", "rounding_minutes": "0", "payment_days": "14", "all_projects": "1"}, max_redirects=0).headers["location"]
    api.post(f"{base}{issued}/lines", form={"description": "Workshop", "detail": "On site, two days", "quantity": "16", "unit": "h", "unit_price": "85", "project": projects["Website relaunch"]})
    business = {"name": "Samuel Wilk", "company": "swilk", "address": "Hauptstraße 1\n12345 Berlin", "tax_number": "12/345/67890", "tax_mode": "standard", "vat_rate": "19"}
    api.post(f"{base}/settings/business", form=business)
    api.post(f"{base}{issued}/lines", form={"description": "Travel", "detail": "", "quantity": "1", "unit": "flat", "unit_price": "120"})
    api.post(f"{base}{issued}/issue")
    hosting = re.findall(r'name="line" value="(\d+)"> Arbeitszeit', api.get(f"{base}{issued}/cancel").text())[0]
    cancellation = api.post(f"{base}{issued}/cancel", form={"line": hosting, "reason": "Hosting is billed yearly"}, max_redirects=0).headers["location"]
    api.post(f"{base}{issued}/paid", form={"paid_date": str(today)})
    # Unset again so the draft's preview shows the missing-data page instead of a PDF download.
    api.post(f"{base}/settings/business", form=business | {"name": ""})
    empty = api.post(f"{base}/invoices", form={"customer": customer, "from": "", "to": "", "grouping": "project", "rounding_minutes": "0", "payment_days": "14", "project": projects["Website relaunch"]}, max_redirects=0).headers["location"]
    seed_open_invoices(api, customer, business)
    # Time after the drafts were made, so the overview shows unbilled time both on a draft and on no invoice.
    last_friday = monday - datetime.timedelta(days=3)
    api.post(f"{base}/entries", form={"project": projects["Website relaunch"], "start": f"{last_friday}T09:00", "end": f"{last_friday}T12:30", "description": "Content migration", "billable": "", "todo": "2"})
    api.post(f"{base}/customers", form={"name": "Nordlicht Logistik GmbH", "address": "Am Hafen 5\n20457 Hamburg", "email": "rechnung@nordlicht.example", "currency": "EUR", "invoice_language": "de", "timezone": "Europe/Berlin", "hourly_rate": "90"})
    other = max(map(int, re.findall(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text())))
    api.post(f"{base}/customers/{other}/projects", form={"name": "Fleet dashboard", "hourly_rate": "", "billable": "on"})
    fleet = re.search(r'href="/projects/(\d+)">', api.get(f"{base}/customers/{other}/projects").text()).group(1)
    api.post(f"{base}/projects/{fleet}/todos", form={"title": "Sketch the dashboard", "description": "", "assignee": "1", "priority": "2", "due_date": ""})
    api.post(f"{base}/entries", form={"project": fleet, "start": f"{last_friday}T13:30", "end": f"{last_friday}T15:00", "description": "Dashboard review", "billable": ""})
    api.post(f"{base}/customers/{other}/projects", form={"name": "Legacy portal", "hourly_rate": "", "billable": "on"})
    legacy = re.search(r'href="/projects/(\d+)">Legacy portal<', api.get(f"{base}/customers/{other}/projects").text()).group(1)
    api.post(f"{base}/projects/{legacy}/todos", form={"title": "Export the old data", "description": "", "assignee": "1", "priority": "1", "due_date": ""})
    api.post(f"{base}/projects/{legacy}/archive")
    quote = api.post(f"{base}/quotes", form={"customer": customer, "project": projects["Website relaunch"], "subject": "Relaunch of the company website with a new shop", "language": "de", "pricing": "fixed", "valid_until": str(today + datetime.timedelta(days=30))}, max_redirects=0).headers["location"]
    for fields in [("Design and implementation", "Landing page, pricing and contact pages", "40", "h", "85", "once"), ("Hosting", "", "1", "month", "29.90", "monthly"), ("Domain", "", "1", "year", "15", "yearly")]:
        api.post(f"{base}{quote}/lines", form=dict(zip(("description", "detail", "quantity", "unit", "unit_price", "interval"), fields)))
    for heading, body in [("Scope", "We build *three* pages:\n\n- Home\n- Pricing\n- Contact"), ("", "We look forward to working with you.")]:
        api.post(f"{base}{quote}/sections", form={"heading": heading, "body": body})
    quote_line = re.search(rf'href="({quote}/lines/\d+)"', api.get(f"{base}{quote}").text()).group(1)
    quote_section = re.search(rf'href="({quote}/sections/\d+)"', api.get(f"{base}{quote}").text()).group(1)
    api.post(f"{base}/quotes", form={"customer": other, "project": "", "subject": "Fleet dashboard", "language": "de", "pricing": "effort", "valid_until": str(today)})
    return customer, projects["Website relaunch"], draft, line, entry, issued, cancellation, empty, legacy, quote, quote_line, quote_section


def seed_open_invoices(api, customer, business):
    api.post(f"{base}/customers", form={"name": "Northwind Trading Ltd", "address": "1 Harbour St\nPortland, ME 04101", "email": "ap@northwind.example", "currency": "USD", "invoice_language": "en", "timezone": "America/New_York", "hourly_rate": "120"})
    northwind = re.findall(r'href="/customers/(\d+)/projects"', api.get(f"{base}/customers").text())[-1]
    api.post(f"{base}/settings/business", form=business)
    today = datetime.date.today()
    for owner, description, quantity, price, overdue_days in [(customer, "Maintenance", "6", "85", 0), (customer, "Consulting", "3.5", "85", 12),
                                                              (northwind, "Integration", "10", "120", 0), (northwind, "Support retainer", "1", "450", 4)]:
        invoice = api.post(f"{base}/invoices", form={"customer": owner, "from": str(today), "to": str(today), "grouping": "project", "rounding_minutes": "0", "payment_days": "14", "all_projects": "1"}, max_redirects=0).headers["location"]
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
        customer, project, draft, line, entry, issued, cancellation, empty, legacy, quote, quote_line, quote_section = seed(api)
        today = datetime.date.today().isoformat()
        templates = [("home", "/"), ("week", "/week"), ("week", "/week?everyone=1"), ("entries", "/entries"),
                     ("entries", f"/entries?day={today}&project={project}"), ("entry", "/entries/new"), ("entry", entry),
                     ("customers", "/customers"), ("customer", "/customers/new"), ("customer", f"/customers/{customer}"),
                     ("projects", f"/customers/{customer}/projects"), ("project", f"/projects/{project}"), ("project", f"/projects/{legacy}"),
                     ("todo", f"/projects/{project}/todos/new"), ("todo", "/todos/1"), ("todos", "/todos"), ("todos", "/todos?assignee=me"), ("todo", "/todos/new"),
                     ("invoices", "/invoices"), ("invoice_new", "/invoices/new"), ("invoice", draft), ("invoice", f"{draft}/preview"),
                     ("invoice_line", f"{draft}/lines/new"), ("invoice_line", line), ("invoice_refresh", f"{draft}/refresh"),
                     ("invoice_issue", f"{draft}/issue"), ("invoice_issued", issued), ("invoice_cancel", f"{issued}/cancel"),
                     ("invoice_issued", cancellation), ("invoice", empty), ("invoice_refresh", f"{empty}/refresh"),
                     ("quotes", "/quotes"), ("quote_new", "/quotes/new"), ("quote", quote), ("quote", "/quotes/2"),
                     ("quote_line", f"{quote}/lines/new"), ("quote_line", quote_line), ("quote_section", f"{quote}/sections/new"), ("quote_section", quote_section),
                     ("quote_duplicate", f"{quote}/duplicate"),
                     ("settings", "/settings"), ("profile", "/profile"), ("error", "/no-such-page")]
        if changed:
            only[:] = [path for name, path in templates if name in changed]
        pages = only or [path for _, path in templates]
        for lang in ("en", "de"):
            page = browser.new_page()
            page.goto(f"{base}/profile")
            page.select_option("select[name=language]", lang)
            page.click("form[action=\"/profile\"] button")
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
                        # Focus styles don't vary by language or scheme, only by which elements a width shows.
                        if lang == "en" and scheme == "light":
                            failures += [f"{viewport} {path}: {p}" for p in unfocusable(page)]
                    if not only:
                        page.goto(base + "/week")
                        page.click("button[aria-haspopup=dialog]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-week-picker.png")
                        failures += [f"{lang} {viewport} {scheme} /week (picker open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.goto(base + draft)
                        page.click(f'form[action="{draft}/delete"] button')
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-confirm-dialog.png")
                        failures += [f"{lang} {viewport} {scheme} {draft} (confirm dialog open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if "/entries" in pages:
                        page.goto(base + "/entries")
                        page.check("[data-select-all]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-entries-selected.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /entries (rows selected): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click("button[data-confirm-many]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-entries-selected-confirm.png")
                        failures += [f"{lang} {viewport} {scheme} /entries (delete confirm open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click("dialog button[value=cancel]")
                        page.evaluate(SELECT_UNSELECTABLE, ["entry-selection", "/entries/"])
                        page.click("button[data-confirm-many]")
                        page.click("dialog button[value=ok]")
                        page.wait_for_selector("main [role=alert]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-entries-blocked.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /entries (delete refused): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if "/invoices" in pages:
                        page.goto(base + "/invoices")
                        page.check("[data-select-all]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-invoices-selected.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /invoices (drafts selected): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click("button[data-confirm-many]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-invoices-selected-confirm.png")
                        failures += [f"{lang} {viewport} {scheme} /invoices (delete confirm open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click("dialog button[value=cancel]")
                        page.evaluate(SELECT_UNSELECTABLE, ["invoice-selection", "/invoices/"])
                        page.click("button[data-confirm-many]")
                        page.click("dialog button[value=ok]")
                        page.wait_for_selector("main [role=alert]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-invoices-blocked.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /invoices (delete refused): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    # Every seeded customer and project is still in use, so the delete is refused and nothing goes.
                    for path in ("/customers", f"/customers/{customer}/projects"):
                        if path not in pages:
                            continue
                        name = re.sub(r"[^a-z0-9]+", "-", path.lower()).strip("-")
                        page.goto(base + path)
                        page.check("[data-select-all]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-{name}-selected.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} {path} (rows selected): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                        page.click("button[data-confirm-many]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-{name}-selected-confirm.png")
                        page.click("dialog button[value=ok]")
                        page.wait_for_selector("main [role=alert]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-{name}-blocked.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} {path} (delete refused): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if not only or "/invoices/new" in only:
                        page.goto(base + "/invoices/new")
                        page.uncheck("input[name=all_projects]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-invoices-new-projects.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /invoices/new (projects shown): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if "/todos" in pages:
                        page.goto(base + "/todos")
                        page.locator("[aria-haspopup=menu]").first.click()
                        page.wait_for_selector("[role=menu]:popover-open")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-todos-status-menu.png")
                        failures += [f"{lang} {viewport} {scheme} /todos (status menu open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    if f"/projects/{project}" in pages:
                        # A column whose status no longer exists, as on a page loaded before the status was deleted.
                        page.goto(f"{base}/projects/{project}")
                        column = page.locator("[data-status]").last
                        column.evaluate("c => c.dataset.status = '0'")
                        # Dispatched rather than dragged by mouse, since the target column may sit outside the board's scrolled view.
                        drag = {"dataTransfer": page.evaluate_handle("new DataTransfer()")}
                        card = page.locator("[draggable]").first
                        card.dispatch_event("dragstart", drag)
                        column.dispatch_event("dragover", drag)
                        column.dispatch_event("drop", drag)
                        card.dispatch_event("dragend", drag)
                        page.wait_for_selector("main [role=alert]")
                        page.screenshot(path=f"{out}/{lang}-{viewport}-{scheme}-project-drag-refused.png", full_page=True)
                        failures += [f"{lang} {viewport} {scheme} /projects/{project} (drag refused): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                    page.close()
        browser.close()
    print(f"screenshots: {out}")
    for f in failures:
        print(f"FAIL {f}")
    sys.exit(1 if failures else 0)


main()
