# Invoice and quote templates

compact-company renders invoices and quotes with [Typst](https://typst.app). A template is a directory with an `invoice.typ` entry point for invoices and a `quote.typ` entry point for quotes; point `invoice.template_dir` at it, or leave it unset to use the bundled `default/` (English and German). A directory without `quote.typ` still renders invoices; previewing or sending a quote then fails with a message naming the missing file.

```toml
[invoice]
template_dir = "/etc/compact-company/invoice-template"
font_paths = ["/etc/compact-company/fonts"]   # optional
```

## Invocation

```
typst compile --root <template_dir> [--font-path <p> …] --input data=<json> <template_dir>/<invoice.typ|quote.typ> <out.pdf>
```

The template reads its data with:

```typst
#let data = json(bytes(sys.inputs.data))
```

Everything about the invoice or quote is in that JSON; a template needs no other source. Files outside `template_dir` can't be read. Fonts come from `font_paths`, the system and Typst's embedded fonts.

A failing compile (a `panic`, a missing font, a syntax error) is shown to the user as Typst reported it, and nothing is issued or sent.

## Invoice contract, version 1

```json
{
  "version": 1,
  "kind": "invoice",
  "draft": false,
  "number": "2026-001",
  "issue_date": "2026-10-27",
  "due_date": "2026-11-10",
  "service_period": { "from": "2026-09-01", "to": "2026-09-30" },
  "language": "de",
  "currency": "EUR",
  "business": {
    "name": "Samuel Wilk",
    "company": "swilk",
    "address": ["Hauptstraße 1", "12345 Berlin"],
    "email": "mail@swilk.example",
    "web": "swilk.example",
    "phone": "030 123",
    "tax_number": "12/345/67890",
    "bank": { "name": "GLS Bank", "iban": "DE02430609671234567890", "bic": "GENODEM1GLS" }
  },
  "tax": { "small_business": false, "vat_rate": 19 },
  "customer": { "number": "K-001", "name": "Genossenschaft Aufwind eG", "address": ["Windweg 3", "54321 Hamburg"] },
  "lines": [
    {
      "description": "Working time",
      "detail": "",
      "quantity": "2.5",
      "unit": "h",
      "unit_price": "85.00",
      "total": "212.50",
      "project": "Website relaunch",
      "entries": [
        { "date": "2026-09-02", "user": "Samuel Wilk", "seconds": 9000, "description": "Kickoff and wireframes" }
      ]
    }
  ],
  "totals": { "net": "212.50", "vat": "40.38", "gross": "252.88" },
  "cancels": null,
  "reason": ""
}
```

| Field | Meaning |
|---|---|
| `version` | Contract version; `1`. A template should refuse versions it doesn't know. |
| `kind` | `invoice` or `cancellation`. |
| `draft` | `true` for a preview: show a draft watermark. |
| `number` | Invoice number; `null` while `draft`. |
| `issue_date`, `due_date` | Issue date (today for a preview) and issue date plus the payment days. |
| `service_period` | When the work was done: from the start date of the first time entry on the invoice to the end date of the last. Without time entries it is set by hand, or else `from` and `to` are the `issue_date`. A cancellation carries the cancelled invoice's. |
| `language` | The customer's invoice language: `en` or `de`. |
| `currency` | ISO 4217 code of every amount. |
| `business` | The sender. Empty strings for unset optional fields; `name`, `address` and `tax_number` are always set. |
| `tax` | `small_business: true` means no VAT under § 19 UStG and `vat_rate` 0; otherwise `vat_rate` is the integer percent applied. A cancellation carries the cancelled invoice's `tax`. |
| `customer` | The recipient. |
| `lines` | In order: by project section, then the lines without a project. `quantity × unit_price = total`, rounded half up to the currency's minor unit. Cancellation lines carry negated quantities and totals. |
| `lines[].project` | The name of the project the line bills, or `null`. Lines of one project are consecutive; a template shows them under a heading with the name (a *project section*, also for a single project) and the lines without a project after all sections, without a heading. Headings carry no amount. |
| `lines[].entries` | The time entries billed by a line, oldest first; empty for free lines and cancellations. `seconds` is the tracked duration before rounding. |
| `totals` | `net` is the sum of line totals, `vat` is rounded once on `net`, `gross = net + vat`. |
| `cancels` | For a cancellation invoice the cancelled invoice's `number` and `issue_date`; otherwise `null`. |
| `reason` | Cancellation reason; may be empty. |

- Dates are ISO `YYYY-MM-DD`, in the customer's timezone.
- Money and quantities are decimal strings with `.` as separator and an optional leading `-` (`"1083.75"`, `"-12.75"`, `"1.5"`); amounts carry the currency's minor-unit digits, quantities up to 3 decimals without trailing zeros. Formatting them is the template's job.
- An issued invoice stores exactly this JSON as its snapshot, so its PDF can be rendered again from it.
- Parse amounts with Typst's `decimal()`, not `float()`, so sums and VAT stay exact.

## Quote contract, version 1

`quote.typ` receives the same `version`, `draft`, `language`, `currency`, `business`, `tax` and `customer` as an invoice, and lines with the invoice's `description`, `detail`, `quantity`, `unit`, `unit_price` and `total`:

```json
{
  "version": 1,
  "kind": "quote",
  "draft": false,
  "number": "A-2026-001",
  "issue_date": "2026-10-27",
  "valid_until": "2026-11-26",
  "subject": "Website relaunch",
  "pricing": "fixed",
  "project": "Website relaunch",
  "language": "de",
  "currency": "EUR",
  "business": { "…": "as for invoices" },
  "tax": { "small_business": false, "vat_rate": 19 },
  "customer": { "number": "K-001", "name": "Genossenschaft Aufwind eG", "address": ["Windweg 3", "54321 Hamburg"] },
  "lines": [
    { "description": "Design", "detail": "Three page layouts", "quantity": "1", "unit": "flat", "unit_price": "1200.00", "interval": "once", "total": "1200.00" },
    { "description": "Hosting", "detail": "", "quantity": "1", "unit": "month", "unit_price": "20.00", "interval": "monthly", "total": "20.00" }
  ],
  "sections": [
    { "heading": "Scope", "body": "We build *three* pages:\n\n+ Home\n+ About\n+ Contact" },
    { "heading": "", "body": "Delivery within four weeks of acceptance." }
  ],
  "totals": {
    "once": { "net": "1200.00", "vat": "228.00", "gross": "1428.00" },
    "monthly": { "net": "20.00", "vat": "3.80", "gross": "23.80" },
    "yearly": null
  }
}
```

| Field | Meaning |
|---|---|
| `kind` | `quote`. |
| `number` | Quote number; `null` while `draft`. |
| `issue_date` | The send date; today for a preview. |
| `valid_until` | The last day the quote can be accepted. |
| `subject` | The quote's subject line; never empty. |
| `pricing` | `fixed`: the total is what will be invoiced. `effort`: the total is an estimate, the time actually spent is invoiced. It changes wording only, never the amounts. |
| `project` | The name of the customer's project the quote is for, or `null` for new work. |
| `language` | The quote's language, `en` or `de`; it may differ from the customer's invoice language. |
| `tax` | The business's tax mode when the quote is rendered. |
| `lines` | In order; may be empty, for a quote that states no price. |
| `lines[].interval` | `once`, `monthly` or `yearly`: a recurring line's `total` is per month or per year. |
| `sections` | In order. `heading` may be empty, for a plain paragraph. `body` is Typst markup; render it with `eval(section.body, mode: "markup")`. |
| `totals` | Per interval: `net` is the sum of that interval's line totals, `vat` is rounded once on it, `gross = net + vat`; `null` for an interval without lines. Intervals are never added together. |

A sent quote stores exactly this JSON as its snapshot, like an issued invoice; the same date and money rules apply.

## Comparing with a reference PDF

To check that a template reproduces an existing PDF (for example one built by hand before switching to compact-company), issue the same invoice in the app, download it from `/invoices/{id}/pdf`, and compare every page's pixels. The PDF bytes differ between renders (creation date), the pixels must not:

```sh
nix shell nixpkgs#poppler-utils nixpkgs#imagemagick -c sh -c '
pdftoppm -r 150 -png reference.pdf ref
pdftoppm -r 150 -png issued.pdf out
ls ref-*.png out-*.png   # same page count
for p in ref-*.png; do echo "$p $(magick compare -metric AE "$p" "out-${p#ref-}" null: 2>&1)"; done
'
```

Run it in the directory holding `reference.pdf` and `issued.pdf`. `AE` is the number of differing pixels; every page must report `0 (0)`.
