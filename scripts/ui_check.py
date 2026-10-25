"""Render every page at desktop and phone width in light and dark, fail on layout breakage, save screenshots.

Usage: ui_check.py <base-url> <screenshot-dir> <chromium>, against a fresh dev-mode instance (it seeds data).
"""

import datetime
import re
import sys

from playwright.sync_api import sync_playwright

base, out, chromium = sys.argv[1:4]

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
    return customer, projects["Website relaunch"], draft, line


def main():
    failures = []
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=chromium)
        api = browser.new_context().request
        customer, project, draft, line = seed(api)
        today = datetime.date.today().isoformat()
        pages = ["/", "/week", "/week?everyone=1", "/entries", f"/entries?day={today}&project={project}", "/entries/new",
                 "/customers", "/customers/new", f"/customers/{customer}", f"/customers/{customer}/projects", f"/projects/{project}",
                 "/invoices", "/invoices/new", draft, f"{draft}/preview", f"{draft}/lines/new", line,
                 "/settings", "/settings/business", "/no-such-page"]
        for viewport, (width, height) in VIEWPORTS.items():
            for scheme in ("light", "dark"):
                page = browser.new_page(viewport={"width": width, "height": height}, color_scheme=scheme)
                for path in pages:
                    page.goto(base + path)
                    name = re.sub(r"[^a-z0-9]+", "-", path.lower()).strip("-") or "home"
                    page.screenshot(path=f"{out}/{viewport}-{scheme}-{name}.png", full_page=True)
                    failures += [f"{viewport} {scheme} {path}: {p}" for p in page.evaluate(FIND_BREAKAGE)]
                page.goto(base + "/week")
                page.click(".picker-trigger")
                page.screenshot(path=f"{out}/{viewport}-{scheme}-week-picker.png")
                failures += [f"{viewport} {scheme} /week (picker open): {p}" for p in page.evaluate(FIND_BREAKAGE)]
                page.close()
        browser.close()
    print(f"screenshots: {out}")
    for f in failures:
        print(f"FAIL {f}")
    sys.exit(1 if failures else 0)


main()
